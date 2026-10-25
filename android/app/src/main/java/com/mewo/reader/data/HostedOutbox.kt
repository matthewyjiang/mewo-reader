package com.mewo.reader.data

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File
import java.io.IOException
import java.util.concurrent.atomic.AtomicReference

/** Who the queue belongs to. A queue is only drained for this exact account. */
@Serializable
internal data class OutboxAccount(val url: String, val username: String)

@Serializable
internal sealed interface OutboxOp {
    val bookId: String

    /** One pending reply. [id] is the comment id the server will store. */
    @Serializable
    @SerialName("reply")
    data class Reply(
        override val bookId: String,
        val postId: String,
        val id: String,
        val text: String,
        val `private`: Boolean = false,
        val createdAt: Long,
        /** Terminal server refusal. Kept until Retry or Delete. */
        val failed: String? = null,
    ) : OutboxOp

    /** Final like intent for one post. At most one per book and post. */
    @Serializable
    @SerialName("like")
    data class Like(
        override val bookId: String,
        val postId: String,
        val liked: Boolean,
    ) : OutboxOp

    /** Latest reading place. At most one per book. */
    @Serializable
    @SerialName("progress")
    data class Progress(
        override val bookId: String,
        val index: Int,
    ) : OutboxOp
}

@Serializable
internal data class OutboxFile(
    val version: Int = OUTBOX_VERSION,
    val account: OutboxAccount,
    val ops: List<OutboxOp> = emptyList(),
)

internal sealed interface Outcome {
    data object Retry : Outcome
    data object Hold401 : Outcome
    data class Refused(val message: String) : Outcome
    data object Gone : Outcome
}

/**
 * Same formula as the comments_private trigger:
 * commenter is not the owner, and the book is not shared and not the sample.
 * The server ORs this in, so a later share cannot publish a reply written here.
 */
internal fun replyIsPrivate(book: BookRecord): Boolean =
    !book.mine && !book.shared && !book.isSample

internal fun HostedSession.outboxAccount(): OutboxAccount? {
    if (!signedIn) return null
    val url = normalizedUrl() ?: return null
    val name = username?.takeIf { it.isNotBlank() } ?: return null
    return OutboxAccount(url, name)
}

/** Likes in [ops] for [bookId] applied onto [base]. Last intent wins. */
internal fun overlayLikes(base: Set<String>, ops: List<OutboxOp>, bookId: String): Set<String> {
    val next = base.toMutableSet()
    for (op in ops) {
        if (op is OutboxOp.Like && op.bookId == bookId) {
            if (op.liked) next.add(op.postId) else next.remove(op.postId)
        }
    }
    return next
}

/** Server thread plus queued replies for this post, in enqueue order. */
internal fun overlayThread(
    base: List<PostComment>,
    ops: List<OutboxOp>,
    bookId: String,
    postId: String,
    me: String,
): List<PostComment> {
    val pending = ops.filterIsInstance<OutboxOp.Reply>()
        .filter { it.bookId == bookId && it.postId == postId }
    if (pending.isEmpty()) return base
    val ids = pending.map { it.id }.toSet()
    val kept = base.filter { it.id !in ids }
    val extra = pending.map { op ->
        PostComment(
            id = op.id,
            username = me,
            text = op.text,
            createdAt = op.createdAt,
            mine = true,
            status = if (op.failed != null) {
                CommentStatus.Failed(op.failed)
            } else {
                CommentStatus.Sending
            },
        )
    }
    return kept + extra
}

/** Pending replies count, and mark the post as mine. */
internal fun overlayIndex(base: CommentIndex, ops: List<OutboxOp>, bookId: String): CommentIndex {
    val replies = ops.filterIsInstance<OutboxOp.Reply>().filter { it.bookId == bookId }
    if (replies.isEmpty()) return base
    val counts = base.counts.toMutableMap()
    val mine = base.mine.toMutableSet()
    for (reply in replies) {
        counts[reply.postId] = (counts[reply.postId] ?: 0) + 1
        mine.add(reply.postId)
    }
    return base.copy(counts = counts, mine = mine)
}

/** Pending progress replaces the server place. One op per book after coalesce. */
internal fun overlayProgress(books: List<BookRecord>, ops: List<OutboxOp>): List<BookRecord> {
    val pending = LinkedHashMap<String, Int>()
    for (op in ops) {
        if (op is OutboxOp.Progress) pending[op.bookId] = op.index
    }
    if (pending.isEmpty()) return books
    return books.map { book ->
        val index = pending[book.id] ?: return@map book
        if (book.progressIndex == index) book else book.copy(progressIndex = index)
    }
}

/**
 * Likes replace in place per book and post. Progress replaces in place per book.
 * Replies always append and keep their order.
 */
internal fun coalesce(ops: List<OutboxOp>, next: OutboxOp): List<OutboxOp> {
    val index = when (next) {
        is OutboxOp.Reply -> -1
        is OutboxOp.Like -> ops.indexOfFirst { op ->
            op is OutboxOp.Like && op.bookId == next.bookId && op.postId == next.postId
        }
        is OutboxOp.Progress -> ops.indexOfFirst { op ->
            op is OutboxOp.Progress && op.bookId == next.bookId
        }
    }
    if (index < 0) return ops + next
    return ops.mapIndexed { at, op -> if (at == index) next else op }
}

/**
 * Transport and throttle errors retry. 401 holds the file. A reply refusal is
 * kept; 403 and 404 on a like or progress are [Outcome.Gone].
 */
internal fun classify(err: Throwable): Outcome {
    if (err is IOException) return Outcome.Retry
    val hosted = err as? HostedException ?: return Outcome.Retry
    val message = hosted.message?.takeIf { it.isNotBlank() } ?: "Could not send."
    return when (hosted.code) {
        401 -> Outcome.Hold401
        408, 429 -> Outcome.Retry
        in 500..599 -> Outcome.Retry
        400, 409 -> Outcome.Refused(message)
        403, 404 -> Outcome.Gone
        else -> Outcome.Retry
    }
}

/**
 * The only caller of hosted like, reply, and progress writes. The file lives
 * next to hosted/, so a 401 wipe of the cache leaves the queue in place.
 * One mutex covers every read and write of that file.
 */
internal class HostedOutbox(
    private val file: File,
    private val sessions: HostedSessionStore,
    private val client: HostedClient,
    private val social: (bookId: String, thread: Pair<String, List<PostComment>>) -> Unit,
    private val scope: CoroutineScope,
    private val waitForRetry: suspend (Long) -> Unit = { delay(it) },
    /**
     * Set for the duration of [social]. The store reads it inside that callback
     * and skips the write when the signed-in account is no longer this one.
     */
    private val socialOwner: AtomicReference<OutboxAccount?> = AtomicReference(null),
) {
    private val json = Json {
        ignoreUnknownKeys = true
        prettyPrint = true
        encodeDefaults = true
    }
    private val mutex = Mutex()
    private val _pending = MutableStateFlow<List<OutboxOp>>(emptyList())
    val pending: StateFlow<List<OutboxOp>> = _pending.asStateFlow()

    private var ops: List<OutboxOp> = emptyList()
    private var loaded = false
    private var epoch = 0
    private var running = false
    private var queued = false
    private var retryMs = OUTBOX_RETRY_MS
    private var retryJob: Job? = null
    private var active: CompletableDeferred<Unit>? = null
    private var activeReplyId: String? = null
    /** Reply ids whose Discard is waiting out the request already on the wire. */
    private val awaitingDiscard = mutableSetOf<String>()
    /** Successful sends those waits still have to delete. Keyed so the next op cannot erase them. */
    private val landed = mutableMapOf<String, Flight>()

    suspend fun enqueue(op: OutboxOp) {
        withContext(Dispatchers.IO) {
            mutex.withLock {
                val acct = account() ?: error(HOSTED_NOT_SIGNED_IN)
                loadIntoMemory(acct)
                ops = coalesce(ops, op)
                write(acct, ops)
                _pending.value = ops
            }
        }
        drain()
    }

    fun drain() {
        scope.launch(Dispatchers.IO) {
            val start = mutex.withLock {
                if (running) {
                    queued = true
                    false
                } else {
                    running = true
                    queued = false
                    true
                }
            }
            if (!start) return@launch
            try {
                drainPass()
            } finally {
                finishDrain()
            }
        }
    }

    suspend fun retry(commentId: String) {
        withContext(Dispatchers.IO) {
            mutex.withLock {
                val acct = account() ?: return@withLock
                loadIntoMemory(acct)
                val next = ops.map { op ->
                    if (op is OutboxOp.Reply && op.id == commentId) op.copy(failed = null) else op
                }
                if (next == ops) return@withLock
                ops = next
                write(acct, ops)
                _pending.value = ops
            }
        }
        drain()
    }

    /**
     * Drops a queued reply. Removal and the in-flight claim share one lock, so a
     * drain cannot pick the reply up after Discard has decided it is still queued.
     * A request already on the wire is waited out; if that request landed, the
     * server row is deleted so Discard cannot leave it up.
     */
    suspend fun discard(commentId: String) {
        val wait = withContext(Dispatchers.IO) {
            mutex.withLock {
                if (activeReplyId == commentId) {
                    awaitingDiscard += commentId
                    active
                } else {
                    removeReply(commentId)
                    null
                }
            }
        }
        val sent = if (wait == null) {
            null
        } else {
            wait.await()
            withContext(Dispatchers.IO) {
                mutex.withLock {
                    awaitingDiscard -= commentId
                    val flight = landed.remove(commentId)
                    removeReply(commentId)
                    flight?.takeIf { it.sent }
                }
            }
        }
        if (sent != null) deleteSent(sent)
    }

    /** Explicit sign-out, or a different account. The in-flight request cannot write after this. */
    suspend fun discardAll() {
        val wait = mutex.withLock {
            epoch += 1
            queued = false
            retryJob?.cancel()
            retryJob = null
            active
        }
        wait?.await()
        withContext(Dispatchers.IO) {
            mutex.withLock {
                ops = emptyList()
                loaded = true
                awaitingDiscard.clear()
                landed.clear()
                _pending.value = emptyList()
                deleteFile()
            }
        }
    }

    fun unsentReplies(): Int = _pending.value.count { it is OutboxOp.Reply }

    /** Signed out keeps the file. A different account deletes it. The same account reloads it. */
    suspend fun attach(session: HostedSession) {
        val resume = withContext(Dispatchers.IO) {
            mutex.withLock {
                // A newer sign-in may already be current. Loading this snapshot would
                // put the previous account's queue back.
                if (sessions.session.value.outboxAccount() != session.outboxAccount()) return@withLock false
                val next = session.outboxAccount()
                val stored = readFile()
                when {
                    next == null -> {
                        epoch += 1
                        queued = false
                        retryJob?.cancel()
                        retryJob = null
                        ops = emptyList()
                        loaded = false
                        _pending.value = emptyList()
                        false
                    }
                    stored != null && stored.account != next -> {
                        epoch += 1
                        queued = false
                        retryJob?.cancel()
                        retryJob = null
                        ops = emptyList()
                        loaded = true
                        _pending.value = emptyList()
                        deleteFile()
                        false
                    }
                    else -> {
                        ops = stored?.ops.orEmpty()
                        loaded = true
                        _pending.value = ops
                        true
                    }
                }
            }
        }
        if (resume) drain()
    }

    private suspend fun finishDrain() {
        while (true) {
            val more = mutex.withLock {
                if (queued) {
                    queued = false
                    true
                } else {
                    running = false
                    false
                }
            }
            if (!more) return
            drainPass()
        }
    }

    private suspend fun drainPass() {
        val acct = account() ?: return
        val passEpoch = mutex.withLock { epoch }
        val stored = mutex.withLock { if (loaded) null else readFile()?.account }
        if (stored != null && stored != acct) return
        while (true) {
            val claim = mutex.withLock {
                if (epoch != passEpoch || account() != acct) return
                if (!loaded) loadIntoMemory(acct)
                val op = ops.firstOrNull { it !is OutboxOp.Reply || it.failed == null } ?: return
                val done = CompletableDeferred<Unit>()
                active = done
                activeReplyId = (op as? OutboxOp.Reply)?.id
                Claim(op, done)
            }
            var failure: Throwable? = null
            var sent: SendResult? = null
            var skipped = false
            var step: Step = Step.Stop
            try {
                val session = sessions.session.value
                if (session.outboxAccount() != acct) {
                    skipped = true
                } else {
                    sent = send(session, claim.op)
                }
            } catch (err: CancellationException) {
                failure = err
                throw err
            } catch (err: Exception) {
                failure = err
            } finally {
                try {
                    step = mutex.withLock {
                        try {
                            val thrown = failure
                            when {
                                thrown is CancellationException -> Step.Stop
                                epoch != passEpoch || account() != acct || skipped -> {
                                    // The queue belongs to someone else now. A Discard that
                                    // is waiting still has to hear that this send landed.
                                    rememberLanded(claim.op, sent)
                                    Step.Stop
                                }
                                thrown != null -> applyFailure(acct, claim.op, thrown)
                                else -> {
                                    applySuccess(acct, claim.op, sent)
                                    Step.Continue
                                }
                            }
                        } finally {
                            if (active === claim.done) {
                                active = null
                                activeReplyId = null
                            }
                        }
                    }
                } finally {
                    claim.done.complete(Unit)
                }
            }
            when (step) {
                Step.Continue -> Unit
                Step.Stop -> return
                is Step.Backoff -> {
                    armRetry(step.wait)
                    return
                }
            }
        }
    }

    private fun applySuccess(acct: OutboxAccount, op: OutboxOp, sent: SendResult?) {
        if (op is OutboxOp.Reply && sent is SendResult.Thread) {
            if (op.id in awaitingDiscard) {
                rememberLanded(op, sent)
            } else {
                publishThread(acct, op.bookId, op.postId to sent.comments)
            }
        }
        val next = without(op)
        if (next != ops) {
            ops = next
            write(acct, ops)
            _pending.value = ops
        }
        retryMs = OUTBOX_RETRY_MS
    }

    private fun applyFailure(acct: OutboxAccount, op: OutboxOp, err: Throwable): Step {
        return when (val outcome = classify(err)) {
            Outcome.Retry -> Step.Backoff(takeBackoff())
            Outcome.Hold401 -> Step.Stop
            is Outcome.Refused -> {
                if (op is OutboxOp.Reply) failReply(acct, op.id, outcome.message)
                else drop(acct, op)
                Step.Continue
            }
            Outcome.Gone -> {
                if (op is OutboxOp.Reply) {
                    val message = err.message?.takeIf { it.isNotBlank() } ?: "Could not send."
                    failReply(acct, op.id, message)
                } else {
                    drop(acct, op)
                }
                Step.Continue
            }
        }
    }

    private fun failReply(acct: OutboxAccount, id: String, message: String) {
        ops = ops.map { op ->
            if (op is OutboxOp.Reply && op.id == id) op.copy(failed = message) else op
        }
        write(acct, ops)
        _pending.value = ops
    }

    private fun drop(acct: OutboxAccount, op: OutboxOp) {
        val next = without(op)
        if (next == ops) return
        ops = next
        write(acct, ops)
        _pending.value = ops
    }

    private fun without(sent: OutboxOp): List<OutboxOp> = when (sent) {
        is OutboxOp.Reply -> ops.filterNot { it is OutboxOp.Reply && it.id == sent.id }
        is OutboxOp.Like -> {
            val index = ops.indexOfFirst { op ->
                op is OutboxOp.Like && op.bookId == sent.bookId && op.postId == sent.postId
            }
            if (index >= 0 && ops[index] == sent) {
                ops.filterIndexed { at, _ -> at != index }
            } else {
                ops
            }
        }
        is OutboxOp.Progress -> {
            val index = ops.indexOfFirst { op ->
                op is OutboxOp.Progress && op.bookId == sent.bookId
            }
            if (index >= 0 && ops[index] == sent) {
                ops.filterIndexed { at, _ -> at != index }
            } else {
                ops
            }
        }
    }

    private fun takeBackoff(): Long {
        val wait = retryMs
        retryMs = (retryMs * 2).coerceAtMost(OUTBOX_RETRY_MAX_MS)
        return wait
    }

    private fun armRetry(wait: Long) {
        retryJob?.cancel()
        retryJob = scope.launch {
            waitForRetry(wait)
            drain()
        }
    }

    private suspend fun deleteSent(flight: Flight) {
        val session = sessions.session.value
        if (session.outboxAccount() == null) return
        val thread = withContext(Dispatchers.IO) {
            sessions.expireOn401(session) {
                client.deleteComment(session, flight.op.bookId, flight.op.postId, flight.op.id)
            }
        }
        if (sessions.session.value.outboxAccount() != session.outboxAccount()) return
        publishThread(
            session.outboxAccount() ?: return,
            flight.op.bookId,
            flight.op.postId to thread,
        )
    }

    private fun rememberLanded(op: OutboxOp, sent: SendResult?) {
        if (op !is OutboxOp.Reply || sent !is SendResult.Thread) return
        if (op.id !in awaitingDiscard) return
        landed[op.id] = Flight(op, sent = true, thread = sent.comments)
    }

    /** Hands the thread to the store only while [acct] is still the signed-in account. */
    private fun publishThread(
        acct: OutboxAccount,
        bookId: String,
        thread: Pair<String, List<PostComment>>,
    ) {
        if (account() != acct) return
        socialOwner.set(acct)
        try {
            if (account() != acct) return
            social(bookId, thread)
        } finally {
            socialOwner.compareAndSet(acct, null)
        }
    }

    private fun send(session: HostedSession, op: OutboxOp): SendResult {
        return when (op) {
            is OutboxOp.Like -> {
                sessions.expireOn401(session) {
                    client.setLike(session, op.bookId, op.postId, op.liked)
                }
                SendResult.Empty
            }
            is OutboxOp.Progress -> {
                sessions.expireOn401(session) {
                    client.patch(session, op.bookId, PatchBook(progressIndex = op.index))
                }
                SendResult.Empty
            }
            is OutboxOp.Reply -> {
                val thread = sessions.expireOn401(session) {
                    client.addComment(
                        session,
                        op.bookId,
                        op.postId,
                        op.text,
                        op.id,
                        op.`private`,
                    )
                }
                SendResult.Thread(thread)
            }
        }
    }

    private fun removeReply(commentId: String) {
        val acct = account() ?: return
        if (!loaded) loadIntoMemory(acct)
        val next = ops.filterNot { it is OutboxOp.Reply && it.id == commentId }
        if (next == ops) return
        ops = next
        write(acct, ops)
        _pending.value = ops
    }

    private fun loadIntoMemory(acct: OutboxAccount) {
        if (loaded) return
        val stored = readFile()
        ops = if (stored?.account == acct) stored.ops else emptyList()
        loaded = true
    }

    private fun account(): OutboxAccount? = sessions.session.value.outboxAccount()

    private fun readFile(): OutboxFile? {
        if (!file.isFile) return null
        noteDisk()
        return runCatching { json.decodeFromString<OutboxFile>(file.readText()) }
            .getOrNull()
            ?.takeIf { it.version == OUTBOX_VERSION }
    }

    private fun write(acct: OutboxAccount, next: List<OutboxOp>) {
        if (next.isEmpty()) {
            deleteFile()
            return
        }
        noteDisk()
        val dir = file.parentFile ?: return
        dir.mkdirs()
        val tmp = File(dir, file.name + ".tmp")
        tmp.writeText(json.encodeToString(OutboxFile(account = acct, ops = next)))
        if (!tmp.renameTo(file)) {
            tmp.copyTo(file, overwrite = true)
            tmp.delete()
        }
    }

    private fun deleteFile() {
        val tmp = File(file.parentFile, file.name + ".tmp")
        if (!file.exists() && !tmp.exists()) return
        noteDisk()
        file.delete()
        tmp.delete()
    }

    private data class Claim(val op: OutboxOp, val done: CompletableDeferred<Unit>)

    private data class Flight(
        val op: OutboxOp.Reply,
        val sent: Boolean,
        val thread: List<PostComment>?,
    )

    private sealed interface Step {
        data object Continue : Step
        data object Stop : Step
        data class Backoff(val wait: Long) : Step
    }

    private sealed interface SendResult {
        data object Empty : SendResult
        data class Thread(val comments: List<PostComment>) : SendResult
    }
}

internal const val HOSTED_OUTBOX_FILE = "hosted-outbox.json"
internal const val OUTBOX_VERSION = 1
internal const val OUTBOX_RETRY_MS = 30_000L
internal const val OUTBOX_RETRY_MAX_MS = 5 * 60_000L
