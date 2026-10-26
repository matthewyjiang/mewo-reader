package com.mewo.reader.data

import android.content.Context
import android.net.ConnectivityManager
import com.mewo.reader.ui.components.discardThenSignOut
import com.mewo.reader.ui.components.discardUnsentTitle
import java.io.File
import java.io.IOException
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.supervisorScope
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowNetwork

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class HostedOutboxTest {
    private val context = RuntimeEnvironment.getApplication()
    private val failures = ConcurrentLinkedQueue<Throwable>()
    private var job = SupervisorJob()
    private var scope = newScope()
    private lateinit var sessionStore: HostedSessionStore

    @Before
    fun setUp() {
        failures.clear()
        job = SupervisorJob()
        scope = newScope()
        context.filesDir.listFiles()?.forEach { it.deleteRecursively() }
        context.getSharedPreferences("mewo_hosted", 0).edit().clear().commit()
        sessionStore = HostedSessionStore(context)
    }

    @After
    fun tearDown() {
        job.cancel()
        scope.cancel()
    }

    @Test
    fun losingAnOlderDefaultDoesNotGoOffline() {
        val manager = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val before = Shadows.shadowOf(manager).networkCallbacks.toSet()
        val connectivity = HostedConnectivity(context)
        try {
            // The shadow stores callbacks in a HashSet, so last() is not the one just registered.
            val callback = (Shadows.shadowOf(manager).networkCallbacks - before).single()
            val older = ShadowNetwork.newInstance(1)
            val current = ShadowNetwork.newInstance(2)
            callback.onAvailable(current)
            assertTrue(connectivity.online.value)
            callback.onLost(older)
            assertTrue(connectivity.online.value)
            callback.onLost(current)
            assertFalse(connectivity.online.value)
        } finally {
            connectivity.close()
        }
    }

    @Test
    fun replyPrivacyMatchesTheServerTrigger() {
        val rows = listOf(
            Triple(false, false, false) to true,
            Triple(false, false, true) to false,
            Triple(false, true, false) to false,
            Triple(false, true, true) to false,
            Triple(true, false, false) to false,
            Triple(true, false, true) to false,
            Triple(true, true, false) to false,
            Triple(true, true, true) to false,
        )
        for ((flags, isPrivate) in rows) {
            val (mine, shared, sample) = flags
            val book = BookRecord(
                id = BOOK,
                title = "Emma",
                author = "Austen",
                handle = "austen",
                importedAt = 1,
                mine = mine,
                shared = shared,
                isSample = sample,
            )
            assertEquals(
                "mine=$mine shared=$shared sample=$sample",
                isPrivate,
                replyIsPrivate(book),
            )
        }
    }

    @Test
    fun statusStaysOutOfCommentEquality() {
        val sent = PostComment(id = "c", username = "ada", text = "hi", createdAt = 1, mine = true)
        val sending = sent.copy(status = CommentStatus.Sending)
        val failed = sent.copy(status = CommentStatus.Failed("nope"))
        assertEquals(sent, sending)
        assertEquals(sent, failed)
        assertEquals(sent.hashCode(), sending.hashCode())
        assertEquals(sent.hashCode(), failed.hashCode())
        assertEquals(setOf(sent), setOf(sent, sending, failed))
        val json = Json.encodeToString(sent.copy(status = CommentStatus.Failed("nope")))
        assertFalse(json.contains("Failed"))
        assertFalse(json.contains("status"))
    }

    @Test
    fun coalesceKeepsRepliesAndFoldsLikesAndProgressInPlace() {
        val likeOn = OutboxOp.Like(BOOK, "p", liked = true)
        val likeOff = likeOn.copy(liked = false)
        val otherLike = OutboxOp.Like(BOOK, "q", liked = true)
        val progress = OutboxOp.Progress(BOOK, 1)
        val moved = OutboxOp.Progress(BOOK, 4)
        val first = reply("a", "one")
        val second = reply("b", "two")
        var ops = emptyList<OutboxOp>()
        ops = coalesce(ops, likeOn)
        ops = coalesce(ops, progress)
        ops = coalesce(ops, first)
        ops = coalesce(ops, otherLike)
        ops = coalesce(ops, likeOff)
        ops = coalesce(ops, moved)
        ops = coalesce(ops, second)
        assertEquals(listOf(likeOff, moved, first, otherLike, second), ops)
    }

    @Test
    fun overlaysApplyPendingWritesAndLeaveServerRowsUntouched() {
        val like = OutboxOp.Like(BOOK, "p", liked = false)
        val extra = OutboxOp.Like(BOOK, "q", liked = true)
        assertEquals(setOf("q"), overlayLikes(setOf("p"), listOf(like, extra), BOOK))
        assertEquals(setOf("p"), overlayLikes(setOf("p"), listOf(like), "other"))

        val pending = reply("new", "queued")
        val refused = pending.copy(id = "bad", text = "no", failed = "That post is gone.")
        val thread = overlayThread(
            base = listOf(
                PostComment("old", "ada", "hi", 1, mine = true),
                PostComment("new", "ada", "server copy", 2, mine = true),
            ),
            ops = listOf(pending, refused),
            bookId = BOOK,
            postId = "p",
            me = "ada",
        )
        assertEquals(listOf("old", "new", "bad"), thread.map { it.id })
        assertEquals("queued", thread[1].text)
        assertTrue(thread[1].status is CommentStatus.Sending)
        assertEquals(CommentStatus.Failed("That post is gone."), thread[2].status)
        assertEquals("ada", thread[1].username)
        assertTrue(thread[1].mine)

        val index = overlayIndex(
            CommentIndex(counts = mapOf("p" to 1), mine = emptySet(), reposts = mapOf("p" to 2)),
            listOf(pending, refused),
            BOOK,
        )
        assertEquals(3, index.count("p"))
        assertTrue(index.commented("p"))
        assertEquals(2, index.repostCount("p"))

        val books = listOf(book(progress = 1), book(id = OTHER, progress = 2))
        val shown = overlayProgress(books, listOf(OutboxOp.Progress(BOOK, 1), OutboxOp.Progress(BOOK, 9)))
        assertEquals(9, shown[0].progressIndex)
        assertTrue(shown[1] === books[1])
    }

    @Test
    fun classifySeparatesRetryHoldRefusalAndGone() {
        assertTrue(classify(IOException("offline")) is Outcome.Retry)
        assertTrue(classify(HostedException("down", 500)) is Outcome.Retry)
        assertTrue(classify(HostedException("slow", 429)) is Outcome.Retry)
        assertTrue(classify(HostedException("expired", 401)) is Outcome.Hold401)
        val refused = classify(HostedException("Too long.", 400))
        assertEquals(Outcome.Refused("Too long."), refused)
        assertEquals(Outcome.Refused("Already there."), classify(HostedException("Already there.", 409)))
        assertTrue(classify(HostedException("missing", 404)) is Outcome.Gone)
        assertTrue(classify(HostedException("forbidden", 403)) is Outcome.Gone)
    }

    @Test
    fun replayingAnAcceptedReplyDropsItOnce() = runBlocking {
        val waits = RetryWaits()
        val client = ScriptedClient()
        var socialBoom = true
        val social = ConcurrentLinkedQueue<List<PostComment>>()
        val outbox = outbox(client, waits) { _, thread ->
            if (socialBoom) {
                socialBoom = false
                throw IOException("died after accept")
            }
            social.add(thread.second)
        }
        val op = reply("c1", "hello")
        client.thread = listOf(PostComment("c1", "ada", "hello", 5, mine = true))
        outbox.enqueue(op)
        withTimeout(5_000) { while (client.attempts.size < 1 || waits.parked() == 0) delay(10) }
        assertEquals(listOf("c1"), outbox.pending.value.filterIsInstance<OutboxOp.Reply>().map { it.id })
        assertTrue(failures.isEmpty())

        waits.release()
        withTimeout(5_000) { while (outbox.pending.value.isNotEmpty()) delay(10) }
        assertEquals(listOf("c1", "c1"), client.attempts.toList())
        assertEquals(listOf("c1"), social.single().map { it.id })
        assertEquals("hello", social.single().single().text)
        assertTrue(failures.isEmpty())
    }

    @Test
    fun aLeftoverReplyFileReplaysWithoutASecondCopy() = runBlocking {
        val client = ScriptedClient()
        val op = reply("c1", "hello")
        writeFile(OutboxFile(account = OutboxAccount(SERVER, "ada"), ops = listOf(op)))
        client.thread = listOf(PostComment("c1", "ada", "hello", 5, mine = true))
        val social = ConcurrentLinkedQueue<String>()
        val outbox = outbox(client) { _, thread -> social.add(thread.second.single().id) }
        outbox.attach(signedIn())
        withTimeout(5_000) { while (outbox.pending.value.isNotEmpty()) delay(10) }
        outbox.drain()
        delay(50)
        assertEquals(listOf("c1"), client.attempts.toList())
        assertEquals(listOf("c1"), social.toList())
        assertFalse(outboxFile().exists())
    }

    @Test
    fun drainSendsInOrderAndSkipsATerminalReplyFailure() = runBlocking {
        val client = ScriptedClient()
        client.commentError = HostedException("That post is gone.", 404)
        val outbox = outbox(client)
        outbox.enqueue(reply("a", "first"))
        withTimeout(5_000) {
            while (outbox.pending.value.filterIsInstance<OutboxOp.Reply>().none { it.failed != null }) delay(10)
        }
        client.commentError = null
        outbox.enqueue(reply("b", "second"))
        withTimeout(5_000) {
            while (outbox.pending.value.filterIsInstance<OutboxOp.Reply>().map { it.id } != listOf("a")) {
                delay(10)
            }
        }
        assertEquals(listOf("a", "b"), client.attempts.toList())
        val left = outbox.pending.value.filterIsInstance<OutboxOp.Reply>().single()
        assertEquals("a", left.id)
        assertEquals("That post is gone.", left.failed)
    }

    @Test
    fun transportErrorsStopTheQueueAndDoubleTheWait() = runBlocking {
        val waits = RetryWaits()
        val client = ScriptedClient()
        client.commentError = IOException("offline")
        val outbox = outbox(client, waits)
        outbox.enqueue(reply("a", "first"))
        withTimeout(5_000) { while (waits.delays.isEmpty() || client.attempts.isEmpty()) delay(10) }
        assertEquals(listOf(OUTBOX_RETRY_MS), waits.delays.toList())
        // A later reply waits out the backoff. It does not jump the queue or poke the server.
        outbox.enqueue(reply("b", "second"))
        delay(40)
        assertEquals(listOf("a"), client.attempts.toList())
        assertEquals(listOf(OUTBOX_RETRY_MS), waits.delays.toList())
        waits.release()
        withTimeout(5_000) { while (waits.delays.size < 2 || client.attempts.size < 2) delay(10) }
        assertEquals(listOf(OUTBOX_RETRY_MS, OUTBOX_RETRY_MS * 2), waits.delays.toList())
        assertEquals(listOf("a", "a"), client.attempts.toList())
        assertEquals(listOf("a", "b"), outbox.pending.value.filterIsInstance<OutboxOp.Reply>().map { it.id })

        client.commentError = null
        waits.release()
        withTimeout(5_000) { while (outbox.pending.value.isNotEmpty()) delay(10) }
        assertEquals(listOf("a", "b"), client.comments.map { it.id })
    }

    @Test
    fun retryWaitDoublesUntilFiveMinutes() = runBlocking {
        val waits = RetryWaits()
        val client = ScriptedClient()
        client.commentError = IOException("offline")
        val outbox = outbox(client, waits)
        outbox.enqueue(reply("a", "first"))
        var expected = OUTBOX_RETRY_MS
        repeat(6) {
            withTimeout(5_000) { while (waits.delays.size < it + 1) delay(10) }
            assertEquals(expected, waits.delays.last())
            expected = (expected * 2).coerceAtMost(OUTBOX_RETRY_MAX_MS)
            waits.release()
        }
        assertEquals(OUTBOX_RETRY_MAX_MS, waits.delays.last())
    }

    @Test
    fun a401KeepsTheFileAndA403OnLikeDropsIt() = runBlocking {
        val client = ScriptedClient()
        client.likeError = HostedException("nope", 401)
        val outbox = outbox(client)
        outbox.enqueue(OutboxOp.Like(BOOK, "p", liked = true))
        withTimeout(5_000) { while (sessionStore.session.value.signedIn) delay(10) }
        val kept = readFile()
        assertEquals("ada", kept.account.username)
        assertTrue(kept.ops.single() is OutboxOp.Like)

        sessionStore.setSignedIn("ada", "token-2")
        client.likeError = HostedException("missing", 404)
        outbox.attach(sessionStore.session.value)
        withTimeout(5_000) { while (outbox.pending.value.isNotEmpty() || outboxFile().exists()) delay(10) }
    }

    @Test
    fun a400OnProgressIsDroppedAndA409OnReplyIsKept() = runBlocking {
        val client = ScriptedClient()
        client.progressError = HostedException("bad index", 400)
        val outbox = outbox(client)
        outbox.enqueue(OutboxOp.Progress(BOOK, 3))
        withTimeout(5_000) { while (outbox.pending.value.isNotEmpty()) delay(10) }
        assertFalse(outboxFile().exists())

        client.commentError = HostedException("Someone else used that id.", 409)
        outbox.enqueue(reply("c", "hi", private = true))
        withTimeout(5_000) {
            while (outbox.pending.value.none { it is OutboxOp.Reply && it.failed != null }) delay(10)
        }
        val failed = outbox.pending.value.filterIsInstance<OutboxOp.Reply>().single()
        assertEquals("Someone else used that id.", failed.failed)
        assertTrue(failed.`private`)
        client.commentError = null
        outbox.retry(failed.id)
        withTimeout(5_000) { while (outbox.pending.value.isNotEmpty()) delay(10) }
        assertEquals(listOf("c"), client.comments.map { it.id })
    }

    @Test
    fun discardWhileSendingDeletesTheServerReply() = runBlocking {
        val client = ScriptedClient()
        client.commentGate = CompletableDeferred()
        val social = AtomicInteger(0)
        val outbox = outbox(client) { _, _ -> social.incrementAndGet() }
        val op = reply("c1", "hello")
        launch { outbox.enqueue(op) }
        withTimeout(5_000) { while (client.commentEntered.get() < 1) delay(10) }
        val discarding = launch { outbox.discard("c1") }
        delay(50)
        assertTrue(discarding.isActive)
        client.thread = listOf(PostComment("c1", "ada", "hello", 5, mine = true))
        client.commentGate?.complete(Unit)
        withTimeout(5_000) { discarding.join() }
        assertEquals(listOf("c1"), client.deleted.toList())
        assertTrue(outbox.pending.value.isEmpty())
        // The accepted reply is not written. Only the delete's thread is.
        assertEquals(1, social.get())
        assertFalse(outboxFile().exists())
    }

    @Test
    fun signOutKeepsTheFileAndAnotherAccountDeletesIt() = runBlocking {
        val client = ScriptedClient()
        client.commentError = IOException("offline")
        val store = open(client)
        val thread = store.addComment(BOOK, "p", "offline note")
        assertEquals(CommentStatus.Sending, thread.single { it.text == "offline note" }.status)
        assertEquals(1, store.unsentReplies())
        assertTrue(outboxFile().exists())
        assertFalse(outboxFile().path.contains("/hosted/"))

        sessionStore.signOut()
        withTimeout(5_000) {
            while (sessionStore.session.value.signedIn || store.unsentReplies() != 0) delay(10)
        }
        assertTrue(outboxFile().exists())
        assertEquals("ada", readFile().account.username)

        sessionStore.setSignedIn("ada", "token-a")
        withTimeout(5_000) { while (store.unsentReplies() != 1) delay(10) }
        client.commentError = null
        store.load()
        withTimeout(5_000) { while (store.unsentReplies() != 0 || outboxFile().exists()) delay(10) }
        assertFalse(outboxFile().exists())

        client.commentError = IOException("offline")
        store.addComment(BOOK, "p", "second")
        withTimeout(5_000) { while (!outboxFile().exists()) delay(10) }
        sessionStore.setSignedIn("bea", "token-b")
        withTimeout(5_000) { while (outboxFile().exists() || store.unsentReplies() != 0) delay(10) }
    }

    @Test
    fun explicitSignOutDiscardDeletesTheFile() = runBlocking {
        val client = ScriptedClient()
        client.commentError = IOException("offline")
        val store = open(client)
        store.addComment(BOOK, "p", "one")
        store.addComment(BOOK, "p", "two")
        assertEquals(2, store.unsentReplies())
        assertEquals("Discard 1 unsent reply?", discardUnsentTitle(1))
        assertEquals("Discard 2 unsent replies?", discardUnsentTitle(2))
        store.discardOutbox()
        assertFalse(outboxFile().exists())
        assertEquals(0, store.unsentReplies())
        sessionStore.signOut()
        withTimeout(5_000) { while (store.library.value.books.isNotEmpty()) delay(10) }
        sessionStore.setSignedIn("ada", "token-a")
        withTimeout(5_000) { while (store.library.value.books.singleOrNull()?.id != BOOK) delay(10) }
        assertEquals(0, store.unsentReplies())
        assertTrue(client.comments.isEmpty())
    }

    @Test
    fun queuedLikesProgressAndRepliesShowBeforeTheServerAnswers() = runBlocking {
        val client = ScriptedClient()
        client.likeError = IOException("offline")
        client.progressError = IOException("offline")
        client.commentError = IOException("offline")
        client.likesError = IOException("offline")
        client.indexError = IOException("offline")
        client.commentsError = IOException("offline")
        val store = open(client)
        assertEquals(setOf("p"), store.toggleLike(BOOK, "p"))
        assertEquals(setOf("p", "q"), store.toggleLike(BOOK, "q"))
        assertEquals(setOf("q"), store.toggleLike(BOOK, "p"))
        store.saveProgress(BOOK, 4)
        store.saveProgress(BOOK, 6)
        val thread = store.addComment(BOOK, "p", "from the train")
        assertEquals("from the train", thread.last().text)
        assertTrue(thread.last().status is CommentStatus.Sending)
        assertEquals(setOf("q"), store.likes(BOOK))
        assertEquals(1, store.commentIndex(BOOK).count("p"))
        assertTrue(store.commentIndex(BOOK).commented("p"))
        assertEquals(6, store.library.value.books.single().progressIndex)
        assertTrue(client.likesCalls.isEmpty())
        assertTrue(client.progressCalls.isEmpty())

        client.likeError = null
        client.progressError = null
        client.commentError = null
        client.likesError = null
        client.indexError = null
        client.commentsError = null
        store.load()
        withTimeout(5_000) {
            while (
                client.progressCalls.toList() != listOf(6) ||
                client.likesCalls.toList() != listOf("p" to false, "q" to true) ||
                client.comments.size != 1 || client.attempts.size < 1
            ) {
                delay(10)
            }
        }
    }

    @Test
    fun aLikeReplacedWhileInFlightSendsTheLaterIntent() = runBlocking {
        val client = ScriptedClient()
        client.likeGate = CompletableDeferred()
        val store = open(client)
        val first = launch { store.toggleLike(BOOK, "p") }
        withTimeout(5_000) { while (client.likeEntered.get() < 1) delay(10) }
        store.toggleLike(BOOK, "p")
        client.likeGate?.complete(Unit)
        withTimeout(5_000) { first.join() }
        withTimeout(5_000) { while (client.likesCalls.toList() != listOf("p" to true, "p" to false)) delay(10) }
    }

    @Test
    fun discardAllDoesNotSendTheReplyStillQueued() = runBlocking {
        val client = ScriptedClient()
        client.commentGate = CompletableDeferred()
        val outbox = outbox(client)
        outbox.enqueue(reply("c1", "one"))
        outbox.enqueue(reply("c2", "two"))
        withTimeout(5_000) { while (client.commentEntered.get() < 1) delay(10) }
        val discarding = launch { outbox.discardAll() }
        delay(40)
        assertTrue(discarding.isActive)
        outbox.drain()
        delay(40)
        client.thread = listOf(PostComment("c1", "ada", "one", 5, mine = true))
        client.commentGate?.complete(Unit)
        withTimeout(5_000) { discarding.join() }
        assertEquals(listOf("c1"), client.attempts.toList())
        assertEquals(listOf("c1"), client.deleted.toList())
        assertTrue(outbox.pending.value.isEmpty())
        assertFalse(outboxFile().exists())
    }

    @Test
    fun deletingASentReplyKeepsTheQueuedRow() = runBlocking {
        val client = ScriptedClient()
        client.commentError = IOException("offline")
        val store = open(client)
        val queued = store.addComment(BOOK, "p", "still queued").last()
        client.thread = listOf(PostComment("old", "ada", "hi", 1, mine = true))
        val after = store.deleteComment(BOOK, "p", "old")
        assertEquals(listOf(queued.id), after.map { it.id })
        assertTrue(after.single().status is CommentStatus.Sending)
        val social = File(context.filesDir, "hosted/$BOOK/social.json")
        assertTrue(social.isFile)
        assertFalse(social.readText().contains(queued.id))
    }

    @Test
    fun aStaleThreadGetCannotHideTheReplyJustSent() = runBlocking {
        val client = ScriptedClient()
        client.commentGate = CompletableDeferred()
        client.commentsGate = CompletableDeferred()
        val stale = listOf(PostComment("old", "ada", "hi", 1, mine = true))
        client.thread = stale
        val store = open(client)
        val returned = CompletableDeferred<List<PostComment>>()
        val adding = launch { returned.complete(store.addComment(BOOK, "p", "from the train")) }
        val queuedId = withTimeout(5_000) {
            var id: String? = null
            while (id == null) {
                id = runCatching { readFile().ops }.getOrNull()
                    ?.filterIsInstance<OutboxOp.Reply>()
                    ?.lastOrNull()
                    ?.id
                if (id == null) delay(10)
            }
            id
        }
        withTimeout(5_000) {
            while (!returned.isCompleted &&
                !(client.commentEntered.get() > 0 && client.commentsEntered.get() > 0)
            ) {
                delay(10)
            }
        }
        client.thread = stale + PostComment(queuedId, "ada", "from the train", 5, mine = true)
        client.commentGate?.complete(Unit)
        val social = File(context.filesDir, "hosted/$BOOK/social.json")
        withTimeout(5_000) {
            while (!social.isFile || queuedId !in social.readText()) delay(10)
        }
        client.thread = stale
        client.commentsGate?.complete(Unit)
        val thread = withTimeout(5_000) { returned.await() }
        adding.join()
        assertTrue(thread.any { it.id == queuedId })
        assertTrue(social.readText().contains(queuedId))
    }

    @Test
    fun backoffWaitsUntilTheProcessIsForegrounded() = runBlocking {
        val waits = RetryWaits()
        val client = ScriptedClient()
        client.commentError = IOException("offline")
        val outbox = outbox(client, waits)
        outbox.enqueue(reply("a", "one"))
        withTimeout(5_000) { while (waits.parked() == 0) delay(10) }
        assertEquals(1, client.attempts.size)
        outbox.setForeground(false)
        withTimeout(5_000) { while (waits.parked() != 0) delay(10) }
        waits.release()
        delay(40)
        assertEquals(1, client.attempts.size)
        outbox.setForeground(true)
        withTimeout(5_000) { while (waits.parked() == 0) delay(10) }
        assertEquals(1, client.attempts.size)
    }

    @Test
    fun enqueueDuringBackoffDoesNotHitTheServerAgain() = runBlocking {
        val waits = RetryWaits()
        val client = ScriptedClient()
        client.commentError = HostedException("down", 500)
        val outbox = outbox(client, waits)
        outbox.enqueue(reply("a", "one"))
        withTimeout(5_000) { while (client.attempts.isEmpty() || waits.delays.isEmpty()) delay(10) }
        outbox.enqueue(reply("b", "two"))
        outbox.enqueue(reply("c", "three"))
        delay(50)
        assertEquals(listOf("a"), client.attempts.toList())
        assertEquals(listOf(OUTBOX_RETRY_MS), waits.delays.toList())
        client.commentError = null
        waits.release()
        withTimeout(5_000) { while (outbox.pending.value.isNotEmpty()) delay(10) }
        assertEquals(listOf("a", "b", "c"), client.comments.map { it.id })
    }

    @Test
    fun twoTogglesLandUnlikedWithoutReadingLikes() = runBlocking {
        val client = ScriptedClient()
        client.likes = emptySet()
        client.likeGate = CompletableDeferred()
        val store = open(client)
        val first = launch { store.toggleLike(BOOK, "p") }
        val second = launch { store.toggleLike(BOOK, "p") }
        withTimeout(2_000) {
            first.join()
            second.join()
        }
        assertEquals(0, client.likeGets.get())
        assertFalse(store.likes(BOOK).contains("p"))
        assertTrue(client.likeGate!!.complete(Unit))
    }

    @Test
    fun aLandedLikeIsRememberedSoTheNextToggleUnlikes() = runBlocking {
        val client = ScriptedClient()
        client.likes = emptySet()
        val store = open(client)
        store.toggleLike(BOOK, "p")
        withTimeout(5_000) {
            while (client.likesCalls.size < 1 || outboxFile().exists()) delay(10)
        }
        val social = File(context.filesDir, "hosted/$BOOK/social.json")
        assertTrue(social.readText().contains("\"p\""))

        store.toggleLike(BOOK, "p")
        withTimeout(5_000) { while (client.likesCalls.size < 2) delay(10) }
        assertEquals(listOf("p" to true, "p" to false), client.likesCalls.toList())
    }

    @Test
    fun otherClientErrorsDoNotBlockTheQueue() = runBlocking {
        assertTrue(classify(HostedException("nope", 405)) is Outcome.Refused)
        assertTrue(classify(HostedException("nope", 422)) is Outcome.Refused)
        assertFalse(classify(HostedException("Could not read the server response.", 200)) is Outcome.Retry)

        val social = AtomicInteger(0)
        val client = ScriptedClient()
        client.likeError = HostedException("nope", 422)
        val outbox = outbox(client) { _, _ -> social.incrementAndGet() }
        outbox.enqueue(OutboxOp.Like(BOOK, "p", liked = true))
        outbox.enqueue(reply("c1", "hi"))
        withTimeout(5_000) {
            while (client.attempts.toList() != listOf("c1") || outbox.pending.value.isNotEmpty()) delay(10)
        }
        assertEquals(1, social.get())

        client.commentError = HostedException("Could not read the server response.", 200)
        outbox.enqueue(reply("c2", "decoded badly"))
        withTimeout(5_000) { while (outbox.pending.value.isNotEmpty()) delay(10) }
        assertEquals(1, social.get())
        assertEquals(listOf("c1", "c2"), client.attempts.toList())
    }

    @Test
    fun drainAfterASilentAccountSwitchDoesNotSendOrRewrite() = runBlocking {
        val client = ScriptedClient()
        client.commentError = IOException("offline")
        val outbox = outbox(client)
        outbox.enqueue(reply("c1", "hello"))
        withTimeout(5_000) { while (client.attempts.isEmpty()) delay(10) }
        val before = outboxFile().readText()
        sessionStore.setSignedIn("bea", "token-b")
        client.commentError = null
        outbox.drain()
        delay(80)
        assertEquals(listOf("c1"), client.attempts.toList())
        assertEquals(before, outboxFile().readText())
        assertEquals("ada", readFile().account.username)
    }

    @Test
    fun discardWhileSendingSkipsDeleteAfterAnAccountSwitch() = runBlocking {
        val client = ScriptedClient()
        client.commentGate = CompletableDeferred()
        val outbox = outbox(client)
        launch { outbox.enqueue(reply("c1", "hello")) }
        withTimeout(5_000) { while (client.commentEntered.get() < 1) delay(10) }
        val discarding = launch { outbox.discard("c1") }
        delay(40)
        assertTrue(discarding.isActive)
        sessionStore.setSignedIn("bea", "token-b")
        client.thread = listOf(PostComment("c1", "ada", "hello", 5, mine = true))
        client.commentGate?.complete(Unit)
        withTimeout(5_000) { discarding.join() }
        assertTrue(client.deleted.isEmpty())
    }

    @Test
    fun signOutFinishesWhenDeletingALandedReplyFails() = runBlocking {
        val client = ScriptedClient()
        client.commentGate = CompletableDeferred()
        client.deleteError = IOException("could not delete")
        val outbox = outbox(client)
        launch { outbox.enqueue(reply("c1", "hello")) }
        withTimeout(5_000) { while (client.commentEntered.get() < 1) delay(10) }
        var signedOut = false
        supervisorScope {
            val leaving = async {
                discardThenSignOut(
                    discard = { outbox.discardAll() },
                    signOut = { signedOut = true },
                )
            }
            delay(50)
            assertTrue(leaving.isActive)
            client.thread = listOf(PostComment("c1", "ada", "hello", 5, mine = true))
            client.commentGate?.complete(Unit)
            var surfaced: Throwable? = null
            try {
                withTimeout(5_000) { leaving.await() }
            } catch (err: IOException) {
                surfaced = err
            }
            assertTrue(signedOut)
            assertEquals("could not delete", surfaced?.message)
        }
        assertTrue(outbox.pending.value.isEmpty())
    }

    @Test
    fun aDiskErrorWhileDrainingStaysInsideTheOutbox() = runBlocking {
        val client = ScriptedClient()
        client.commentGate = CompletableDeferred()
        val dir = File(context.filesDir, "ro-outbox").apply { mkdirs() }
        val file = File(dir, HOSTED_OUTBOX_FILE)
        sessionStore.setBaseUrl(SERVER)
        sessionStore.setSignedIn("ada", "token-a")
        val outbox = HostedOutbox(
            file = file,
            sessions = sessionStore,
            client = client,
            social = { _, _ -> },
            scope = scope,
            waitForRetry = { delay(it) },
        )
        outbox.enqueue(reply("c1", "one"))
        outbox.enqueue(reply("c2", "two"))
        withTimeout(5_000) { while (client.commentEntered.get() < 1) delay(10) }
        assertTrue(dir.setWritable(false, false))
        try {
            client.thread = listOf(PostComment("c1", "ada", "one", 5, mine = true))
            client.commentGate?.complete(Unit)
            withTimeout(5_000) { while (client.attempts.isEmpty()) delay(10) }
            delay(200)
            assertTrue(failures.toString(), failures.isEmpty())
        } finally {
            dir.setWritable(true, false)
        }
    }

    @Test
    fun aTornOutboxFileIsNotReplaced() = runBlocking {
        outboxFile().writeText("{not json")
        val client = ScriptedClient()
        val outbox = outbox(client)
        var threw: IOException? = null
        try {
            outbox.enqueue(reply("c1", "hi"))
        } catch (err: IOException) {
            threw = err
        }
        assertTrue(threw != null)
        assertEquals("{not json", outboxFile().readText())
        assertTrue(client.attempts.isEmpty())
    }

    @Test
    fun aCorruptOutboxIsQuarantinedAndAttachContinues() = runBlocking {
        outboxFile().writeText("{not json")
        val client = ScriptedClient()
        val outbox = outbox(client)
        outbox.attach(sessionStore.session.value)
        val bad = File(outboxFile().parentFile, "${outboxFile().name}.bad")
        assertTrue(bad.isFile)
        assertEquals("{not json", bad.readText())
        assertFalse(outboxFile().exists())
        outbox.enqueue(reply("c1", "hi"))
        withTimeout(5_000) {
            while (client.attempts.isEmpty() || outbox.pending.value.isNotEmpty()) delay(10)
        }
        assertEquals(listOf("c1"), client.attempts.toList())

        outboxFile().writeText("{again")
        outbox.attach(sessionStore.session.value)
        val bads = outboxFile().parentFile!!.listFiles { child -> child.name.endsWith(".bad") }!!
        assertEquals(listOf(bad.name), bads.map { it.name })
        assertEquals("{again", bad.readText())
    }

    @Test
    fun aQuarantinedOutboxIsDeletedOnSignOutAndAccountSwitch() = runBlocking {
        val client = ScriptedClient()
        val store = open(client)
        val bad = File(outboxFile().parentFile, "${outboxFile().name}.bad")
        bad.writeText("ada's unsent reply")
        store.discardOutbox()
        assertFalse(bad.exists())

        bad.writeText("ada's unsent reply")
        sessionStore.signOut()
        withTimeout(5_000) { while (bad.exists() || sessionStore.session.value.signedIn) delay(10) }
        assertFalse(bad.exists())

        bad.writeText("ada's unsent reply")
        sessionStore.setSignedIn("ada", "token-a")
        withTimeout(5_000) {
            while (store.library.value.books.singleOrNull()?.id != BOOK) delay(10)
        }
        assertTrue(bad.exists())
        sessionStore.setSignedIn("bea", "token-b")
        withTimeout(5_000) { while (bad.exists()) delay(10) }
        assertFalse(bad.exists())
    }

    @Test
    fun anAccountSwitchDropsPlacesThePreviousAccountLanded() = runBlocking {
        val client = ScriptedClient()
        val store = open(client)
        store.saveProgress(BOOK, 9)
        withTimeout(5_000) { while (outboxFile().exists()) delay(10) }
        assertEquals(9, store.library.value.books.single().progressIndex)
        ShelfCache(File(context.filesDir, "hosted-shelves"), Json { ignoreUnknownKeys = true })
            .write(SERVER, "bea", listOf(book(progress = 2)))

        val before = store.library.value
        client.libraryGate = CompletableDeferred()
        sessionStore.setSignedIn("bea", "token-b")
        try {
            withTimeout(5_000) {
                while (store.library.value === before || store.library.value.books.isEmpty()) delay(10)
            }
            assertEquals(2, store.library.value.books.single().progressIndex)
        } finally {
            client.libraryGate?.complete(Unit)
        }
    }

    @Test
    fun aCorruptOutboxWhoseRenameFailsStartsEmpty() = runBlocking {
        val path = outboxFile()
        path.writeText("{not json")
        val stuck = object : File(path.absolutePath) {
            override fun renameTo(dest: File): Boolean = false
        }
        val client = ScriptedClient()
        sessionStore.setBaseUrl(SERVER)
        sessionStore.setSignedIn("ada", "token-a")
        val outbox = HostedOutbox(
            file = stuck,
            sessions = sessionStore,
            client = client,
            social = { _, _ -> },
            scope = scope,
            waitForRetry = { delay(it) },
        )
        outbox.attach(sessionStore.session.value)
        assertTrue(outbox.pending.value.isEmpty())
        assertFalse(path.exists())
        outbox.enqueue(reply("c1", "hi"))
        withTimeout(5_000) { while (client.attempts.isEmpty()) delay(10) }
        assertEquals(listOf("c1"), client.attempts.toList())
    }

    @Test
    fun queuedReplyAlreadyInTheThreadDoesNotDoubleTheCount() = runBlocking {
        val client = ScriptedClient()
        client.commentError = IOException("offline")
        client.indexError = IOException("offline")
        val store = open(client)
        val queued = store.addComment(BOOK, "p", "hi").last()
        val dir = File(context.filesDir, "hosted/$BOOK").apply { mkdirs() }
        val cache = SocialCache(
            index = CommentIndex(counts = mapOf("p" to 1), mine = setOf("p")),
            threads = mapOf(
                "p" to listOf(PostComment(queued.id, "ada", "hi", queued.createdAt, mine = true)),
            ),
        )
        File(dir, "social.json").writeText(Json.encodeToString(cache))
        assertEquals(1, store.commentIndex(BOOK).count("p"))
    }

    @Test
    fun aShelfRefreshKeepsProgressThatLandedWhileItWasInFlight() = runBlocking {
        val client = ScriptedClient()
        client.progressGate = CompletableDeferred()
        val store = open(client)
        client.libraryGate = CompletableDeferred()
        val saving = launch { store.saveProgress(BOOK, 6) }
        withTimeout(5_000) { while (client.progressEntered.get() < 1) delay(10) }
        val librarySeen = client.libraryEntered.get()
        val loading = launch { store.load() }
        withTimeout(5_000) { while (client.libraryEntered.get() <= librarySeen) delay(10) }
        client.progressGate?.complete(Unit)
        withTimeout(5_000) { while (outboxFile().exists()) delay(10) }
        client.libraryGate?.complete(Unit)
        withTimeout(5_000) {
            saving.join()
            loading.join()
        }
        assertEquals(6, store.library.value.books.single().progressIndex)
    }

    @Test
    fun aLaterShelfRefreshReplacesConfirmedProgress() = runBlocking {
        val client = ScriptedClient()
        val store = open(client)
        store.saveProgress(BOOK, 10)
        withTimeout(5_000) { while (outboxFile().exists()) delay(10) }
        client.shelf[0] = book(progress = 20)
        store.load()
        withTimeout(5_000) {
            while (store.loadState.value.isRefreshing()) delay(10)
        }
        assertEquals(20, store.library.value.books.single().progressIndex)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun outboxDiskWorkStaysOffTheMainThread() = runBlocking {
        val main = java.util.concurrent.Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "mewo-main")
        }
        Dispatchers.setMain(main.asCoroutineDispatcher())
        try {
            val client = ScriptedClient()
            client.commentError = IOException("offline")
            val store = open(client)
            hostedDiskThreads.clear()
            withContext(Dispatchers.Main) {
                store.addComment(BOOK, "p", "note")
            }
            val names = hostedDiskThreads.toList()
            assertTrue(names.toString(), names.isNotEmpty() && names.none { it.startsWith("mewo-main") })
        } finally {
            Dispatchers.resetMain()
            main.shutdown()
        }
    }

    private fun newScope(): CoroutineScope {
        val handler = CoroutineExceptionHandler { _, err ->
            if (err !is kotlinx.coroutines.CancellationException) failures.add(err)
        }
        return CoroutineScope(job + Dispatchers.IO + handler)
    }

    private fun outbox(
        client: ScriptedClient,
        waits: RetryWaits = RetryWaits(),
        social: (String, Pair<String, List<PostComment>>) -> Unit = { _, _ -> },
    ): HostedOutbox {
        if (!sessionStore.session.value.signedIn) {
            sessionStore.setBaseUrl(SERVER)
            sessionStore.setSignedIn("ada", "token-a")
        }
        return HostedOutbox(
            file = outboxFile(),
            sessions = sessionStore,
            client = client,
            social = social,
            scope = scope,
            waitForRetry = { wait -> waits.park(wait) },
        )
    }

    private fun open(client: ScriptedClient): HostedLibraryStore {
        client.shelf.clear()
        client.shelf.add(book())
        sessionStore.setBaseUrl(SERVER)
        val store = HostedLibraryStore(context, EpubOpener(context), sessionStore, client, scope)
        runBlocking {
            sessionStore.setSignedIn("ada", "token-a")
            withTimeout(5_000) {
                while (store.library.value.books.singleOrNull()?.id != BOOK) delay(10)
            }
        }
        return store
    }

    private fun signedIn(): HostedSession {
        sessionStore.setBaseUrl(SERVER)
        sessionStore.setSignedIn("ada", "token-a")
        return sessionStore.session.value
    }

    private fun outboxFile() = File(context.filesDir, HOSTED_OUTBOX_FILE)

    private fun writeFile(file: OutboxFile) {
        val json = Json { encodeDefaults = true; prettyPrint = true }
        outboxFile().writeText(json.encodeToString(file))
    }

    private fun readFile(): OutboxFile {
        val json = Json { ignoreUnknownKeys = true }
        return json.decodeFromString(outboxFile().readText())
    }

    private fun reply(id: String, text: String, private: Boolean = false) = OutboxOp.Reply(
        bookId = BOOK,
        postId = "p",
        id = id,
        text = text,
        `private` = private,
        createdAt = 5,
    )

    private fun book(id: String = BOOK, progress: Int = 0) = BookRecord(
        id = id,
        title = id,
        author = "Austen",
        handle = "austen",
        importedAt = 1,
        postCount = 8,
        progressIndex = progress,
        mine = true,
        shared = false,
        ownerUsername = "ada",
    )

    private class RetryWaits {
        val delays = ConcurrentLinkedQueue<Long>()
        private val gates = ConcurrentLinkedQueue<CompletableDeferred<Unit>>()

        suspend fun park(wait: Long) {
            val gate = CompletableDeferred<Unit>()
            gates.add(gate)
            delays.add(wait)
            try {
                gate.await()
            } finally {
                gates.remove(gate)
            }
        }

        fun parked(): Int = gates.size

        fun release() {
            val pending = gates.toList()
            pending.forEach { gates.remove(it) }
            pending.forEach { it.complete(Unit) }
        }
    }

    private class ScriptedClient : HostedClient() {
        val shelf = mutableListOf<BookRecord>()
        var likes = setOf("kept")
        var index = CommentIndex(counts = mapOf("p" to 1))
        var thread = listOf(PostComment("old", "ada", "hi", 1, mine = true))
        var likeError: Exception? = null
        var progressError: Exception? = null
        var commentError: Exception? = null
        var deleteError: Exception? = null
        var likesError: Exception? = null
        var indexError: Exception? = null
        var commentsError: Exception? = null
        var likeGate: CompletableDeferred<Unit>? = null
        var commentGate: CompletableDeferred<Unit>? = null
        var commentsGate: CompletableDeferred<Unit>? = null
        var progressGate: CompletableDeferred<Unit>? = null
        var libraryGate: CompletableDeferred<Unit>? = null
        val likeEntered = AtomicInteger(0)
        val commentEntered = AtomicInteger(0)
        val commentsEntered = AtomicInteger(0)
        val progressEntered = AtomicInteger(0)
        val libraryEntered = AtomicInteger(0)
        val likeGets = AtomicInteger(0)
        val likesCalls = ConcurrentLinkedQueue<Pair<String, Boolean>>()
        val progressCalls = ConcurrentLinkedQueue<Int>()
        val attempts = ConcurrentLinkedQueue<String>()
        val comments = ConcurrentLinkedQueue<PostComment>()
        val deleted = ConcurrentLinkedQueue<String>()

        override fun library(session: HostedSession): LibrarySnapshot {
            libraryEntered.incrementAndGet()
            libraryGate?.let { runBlocking { it.await() } }
            return LibrarySnapshot(shelf.toList())
        }

        override fun posts(
            session: HostedSession,
            id: String,
            onProgress: (Float?) -> Unit,
        ): List<FeedPost> = emptyList()

        override fun downloadCover(session: HostedSession, id: String, dest: File): Boolean = false

        override fun likes(session: HostedSession, id: String): Set<String> {
            likeGets.incrementAndGet()
            likesError?.let { throw it }
            return likes
        }

        override fun commentIndex(session: HostedSession, id: String): CommentIndex {
            indexError?.let { throw it }
            return index
        }

        override fun comments(session: HostedSession, id: String, postId: String): List<PostComment> {
            commentsEntered.incrementAndGet()
            commentsGate?.let { runBlocking { it.await() } }
            commentsError?.let { throw it }
            return thread
        }

        override fun setLike(session: HostedSession, id: String, postId: String, on: Boolean): Set<String> {
            likeEntered.incrementAndGet()
            likeGate?.let { runBlocking { it.await() } }
            likeError?.let { throw it }
            likesCalls.add(postId to on)
            likes = if (on) likes + postId else likes - postId
            return likes
        }

        override fun patch(session: HostedSession, id: String, body: PatchBook): BookRecord {
            progressEntered.incrementAndGet()
            progressGate?.let { runBlocking { it.await() } }
            progressError?.let { throw it }
            val index = body.progressIndex ?: 0
            progressCalls.add(index)
            val current = shelf.firstOrNull { it.id == id } ?: bookRecord(id)
            return current.copy(progressIndex = index)
        }

        override fun addComment(
            session: HostedSession,
            id: String,
            postId: String,
            text: String,
            commentId: String,
            `private`: Boolean,
        ): List<PostComment> {
            commentEntered.incrementAndGet()
            commentGate?.let { runBlocking { it.await() } }
            attempts.add(commentId)
            commentError?.let { throw it }
            val comment = PostComment(commentId, "ada", text, 5, mine = true)
            comments.add(comment)
            if (thread.none { it.id == commentId }) thread = thread + comment
            return thread
        }

        override fun deleteComment(
            session: HostedSession,
            id: String,
            postId: String,
            commentId: String,
        ): List<PostComment> {
            deleted.add(commentId)
            deleteError?.let { throw it }
            thread = thread.filterNot { it.id == commentId }
            return thread
        }

        private fun bookRecord(id: String) = BookRecord(
            id = id,
            title = id,
            author = "Austen",
            handle = "austen",
            importedAt = 1,
        )
    }

    companion object {
        private const val SERVER = "https://mewo.example"
        private const val BOOK = "11111111-1111-1111-1111-111111111111"
        private const val OTHER = "22222222-2222-2222-2222-222222222222"
    }
}
