package com.mewo.reader.data

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.isActive
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import java.io.File
import java.io.IOException
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/**
 * Hosted shelf. Talks HTTP. The server parses each book, so every client
 * reads the same posts and post ids. Readium here only reads title, author,
 * and cover for upload. Cache lives under filesDir/hosted so it never mixes
 * with the local shelf.
 */
class HostedLibraryStore(
    private val context: Context,
    private val opener: EpubOpener,
    private val sessions: HostedSessionStore,
    private val client: HostedClient,
    private val scope: CoroutineScope,
) : LibraryStore {
    override val hostedSocial: Boolean = true
    private val json = Json {
        ignoreUnknownKeys = true
        prettyPrint = true
    }
    private val root = File(context.filesDir, "hosted").apply { mkdirs() }
    private val shelfCache = ShelfCache(File(context.filesDir, "hosted-shelves"), json)
    private val mutex = Mutex()
    private var epoch = 0
    private var inFlight: CompletableDeferred<Unit>? = null
    private var inFlightToken: String? = null
    private var refreshJob: Job? = null
    private val importedWhileRefreshing = mutableSetOf<String>()
    private val _library = MutableStateFlow(LibrarySnapshot())
    override val library: StateFlow<LibrarySnapshot> = _library.asStateFlow()
    private val _load = MutableStateFlow<LibraryLoad>(LibraryLoading)
    override val loadState: StateFlow<LibraryLoad> = _load.asStateFlow()
    private val live = MutableStateFlow<Map<String, OfflineState>>(emptyMap())
    private val onDisk = ConcurrentHashMap.newKeySet<String>()
    private val bookJobs = ConcurrentHashMap<String, Job>()
    private val feedJobs = ConcurrentHashMap.newKeySet<Job>()
    private val stalePosts = ConcurrentHashMap.newKeySet<String>()
    private val socialLock = Any()
    private val offlineLock = Any()

    @Volatile
    private var batch: Job? = null
    private val _offline = MutableStateFlow<Map<String, OfflineState>>(emptyMap())
    override val offline: StateFlow<Map<String, OfflineState>> = _offline.asStateFlow()
    private val _summary = MutableStateFlow(OfflineSummary(0, 0, false))
    override val offlineSummary: StateFlow<OfflineSummary> = _summary.asStateFlow()
    private val shares = HostedShareToggle(
        books = { _library.value.books },
        replace = { replaceBook(it) },
    )
    private val _replies = MutableStateFlow(0)
    override val replyRevision: StateFlow<Int> = _replies.asStateFlow()
    /** Account the outbox is publishing a thread for. Null outside that callback. */
    private val socialOwner = AtomicReference<OutboxAccount?>(null)
    private val outbox = HostedOutbox(
        file = File(context.filesDir, HOSTED_OUTBOX_FILE),
        sessions = sessions,
        client = client,
        social = { bookId, thread ->
            val safe = safeBookId(bookId) ?: return@HostedOutbox
            val owner = socialOwner.get() ?: return@HostedOutbox
            synchronized(socialLock) {
                if (sessions.session.value.outboxAccount() != owner) return@synchronized
                if (socialOwner.get() != owner) return@synchronized
                writeSocial(safe) { cache ->
                    cache.copy(
                        threads = cache.threads + thread,
                        index = cache.index.afterThread(thread.first, thread.second),
                    )
                }
            }
        },
        scope = scope,
        socialOwner = socialOwner,
    )
    private val connectivity = HostedConnectivity(context)

    init {
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) {
                outbox.setForeground(true)
            }

            override fun onStop(owner: LifecycleOwner) {
                outbox.setForeground(false)
            }
        })
        scope.launch {
            outbox.pending
                .map { ops -> ops.filterIsInstance<OutboxOp.Reply>() }
                .distinctUntilChanged()
                .collect { _replies.value = _replies.value + 1 }
        }
        scope.launch {
            var online = connectivity.online.value
            connectivity.online.collect { now ->
                val regained = now && !online
                online = now
                if (regained) outbox.drain(bypassBackoff = true)
            }
        }
        scope.launch {
            var previous = sessions.session.value
            // Don't load inside the collector. A blocked refresh must not delay
            // the next account's clear, or the old shelf stays on screen.
            sessions.session.collect { session ->
                val urlChanged = previous.normalizedUrl() != session.normalizedUrl()
                val userChanged = previous.username != session.username
                val signedOut = previous.signedIn && !session.signedIn
                if (previous.signedIn && (signedOut || urlChanged || userChanged)) {
                    refreshJob?.cancel()
                    clearShelf(previous)
                    if (signedOut || urlChanged) {
                        wipeCache()
                    } else {
                        cancelDownloads()
                        clearSocialFiles()
                    }
                    mutex.withLock {
                        _library.value = LibrarySnapshot()
                        _load.value = if (session.signedIn) LibraryLoading else LibraryLoaded(emptyList(), false)
                    }
                    synchronized(offlineLock) {
                        live.value = emptyMap()
                        onDisk.clear()
                        publishOfflineLocked()
                    }
                }
                previous = session
                // Attach before the refresh so a resumed queue is in memory for the
                // shelf overlay. Launch it: a drain must not delay the next account's clear.
                val attached = session
                launch {
                    outbox.attach(attached)
                    runCatching { load() }
                }
            }
        }
    }

    override suspend fun load() {
        val token = sessions.session.value.token
        val existing = mutex.withLock {
            if (inFlight != null && inFlightToken == token) inFlight else null
        }
        if (existing != null) {
            existing.await()
            return
        }
        coroutineScope {
            val flight = CompletableDeferred<Unit>()
            val job = coroutineContext[Job]
            val joined = mutex.withLock {
                val winner = if (inFlight != null && inFlightToken == token) inFlight else null
                if (winner == null) {
                    refreshJob?.cancel()
                    refreshJob = job
                    inFlight = flight
                    inFlightToken = token
                }
                winner
            }
            if (joined != null) {
                joined.await()
                return@coroutineScope
            }
            try {
                loadBody()
                flight.complete(Unit)
            } catch (err: Throwable) {
                flight.completeExceptionally(err)
                throw err
            } finally {
                mutex.withLock {
                    if (inFlight === flight) {
                        inFlight = null
                        inFlightToken = null
                        if (refreshJob === job) refreshJob = null
                    }
                }
            }
        }
    }

    override fun dismissLoadError() {
        _load.value = reduceLibraryLoad(_load.value, LibraryEvent.Dismissed)
    }

    private suspend fun loadBody() {
        val session = sessions.session.value
        if (!session.signedIn) {
            mutex.withLock {
                _library.value = LibrarySnapshot()
                _load.value = LibraryLoaded(emptyList(), refreshing = false)
            }
            return
        }
        val url = session.normalizedUrl() ?: return
        val user = session.username.orEmpty()
        val cached = withContext(Dispatchers.IO) { shelfCache.read(url, user) }
        if (sessions.session.value.token != session.token) return
        var showCached: List<BookRecord>? = null
        val start = mutex.withLock {
            if (sessions.session.value.token != session.token) return@withLock null
            if (cached != null && _library.value.books.isEmpty()) {
                outbox.reconcileProgress(cached)
                val snap = outbox.view()
                val shown = overlayProgress(cached, snap.ops, snap.progress)
                _library.value = LibrarySnapshot(shown)
                _load.value = reduceLibraryLoad(_load.value, LibraryEvent.Cached(shown))
                showCached = shown
            }
            _load.value = reduceLibraryLoad(_load.value, LibraryEvent.RefreshStarted)
            importedWhileRefreshing.clear()
            epoch
        } ?: return
        showCached?.let { rescan(it) }
        try {
            val snap = withContext(Dispatchers.IO) {
                try {
                    val remote = client.library(session)
                    remote.books.forEach { book ->
                        val id = safeBookId(book.id) ?: return@forEach
                        ensureCover(session, id)
                    }
                    remote.copy(books = remote.books.filter { safeBookId(it.id) != null })
                } catch (err: HostedException) {
                    if (err.code == 401 && sessions.session.value.token == session.token) {
                        sessions.signOut()
                    }
                    throw err
                }
            }
            if (sessions.session.value.token != session.token) return
            mutex.withLock {
                if (sessions.session.value.token != session.token) return@withLock
                val serverBooks = snap.books
                val merged = if (epoch == start) {
                    serverBooks
                } else {
                    val extras = _library.value.books.filter { local ->
                        local.id in importedWhileRefreshing && serverBooks.none { it.id == local.id }
                    }
                    (serverBooks + extras).sortedByDescending { it.importedAt }
                }
                publish(merged, LibraryEvent.RefreshSucceeded(merged), session)
            }
            if (sessions.session.value.token == session.token) outbox.drain(bypassBackoff = true)
        } catch (err: Exception) {
            if (err is CancellationException) throw err
            if (sessions.session.value.token == session.token) {
                mutex.withLock {
                    _load.value = reduceLibraryLoad(
                        _load.value,
                        LibraryEvent.RefreshFailed(err.message ?: "Could not load the library."),
                    )
                }
            }
            throw err
        }
    }

    private suspend fun clearShelf(session: HostedSession) {
        val url = session.normalizedUrl() ?: return
        val user = session.username ?: return
        withContext(Dispatchers.IO) {
            noteDisk()
            shelfCache.clear(url, user)
            FileHomeFeedCache(File(context.filesDir, "home-feeds")).clear("$url\n$user")
        }
    }

    override suspend fun importFromUri(uri: Uri): BookRecord = withContext(Dispatchers.IO) {
        val session = requireSession()
        val scratch = File(root, "tmp-${UUID.randomUUID()}").apply { mkdirs() }
        try {
            val epub = File(scratch, "book.epub")
            context.contentResolver.openInputStream(uri)?.use { input ->
                epub.outputStream().use { input.copyTo(it) }
            } ?: error("Could not read that file")
            ingest(session, epub, isSample = false, scratch = scratch)
        } finally {
            scratch.deleteRecursively()
        }
    }

    override suspend fun importSample(): BookRecord = withContext(Dispatchers.IO) {
        val session = requireSession()
        val existing = _library.value.books.firstOrNull { it.isSample }
        if (existing != null) return@withContext existing
        val scratch = File(root, "tmp-${UUID.randomUUID()}").apply { mkdirs() }
        try {
            val epub = File(scratch, "book.epub")
            context.assets.open("sample.epub").use { input ->
                epub.outputStream().use { input.copyTo(it) }
            }
            ingest(session, epub, isSample = true, scratch = scratch)
        } finally {
            scratch.deleteRecursively()
        }
    }

    override suspend fun delete(id: String) {
        val session = requireSession()
        val safe = requireBookId(id)
        val job = bookJobs[safe]
        job?.cancel()
        job?.join()
        withContext(Dispatchers.IO) {
            sessions.expireOn401(session) { client.delete(session, safe) }
            bookDir(safe).deleteRecursively()
        }
        synchronized(offlineLock) {
            onDisk.remove(safe)
            live.value = live.value - safe
            publishOfflineLocked()
        }
        mutex.withLock {
            epoch += 1
            val books = _library.value.books.filterNot { it.id == safe }
            publish(books, LibraryEvent.BooksReplaced(books))
        }
    }

    override suspend fun setShared(id: String, shared: Boolean) {
        val session = requireSession()
        val safe = requireBookId(id)
        shares.setShared(safe, shared) {
            withContext(Dispatchers.IO) {
                sessions.expireOn401(session) { client.setShared(session, safe, shared) }
            }
        }
    }

    override suspend fun book(id: String): BookRecord? = _library.value.books.find { it.id == id }

    override fun coverFile(id: String): File? {
        val safe = safeBookId(id) ?: return null
        return File(bookDir(safe), "cover.jpg")
    }

    override suspend fun feed(id: String): List<FeedPost> = withContext(Dispatchers.IO) {
        val session = requireSession()
        val safe = requireBookId(id)
        if (!savedTextAllowed(_load.value, safe)) {
            error("That book is not on your shelf.")
        }
        val job = coroutineContext[Job]
        if (job != null) feedJobs.add(job)
        try {
            cachedFeed(safe)?.let { return@withContext it }
            val posts = runInterruptible {
                sessions.expireOn401(session) { client.posts(session, safe) }
            }
            ensureActive()
            if (!writeCachedPosts(safe, posts, session)) return@withContext posts
            posts
        } finally {
            if (job != null) feedJobs.remove(job)
        }
    }

    override suspend fun setDownloaded(id: String, on: Boolean) {
        changeDownload(id, on, fromBatch = false)
    }

    /**
     * The caller's scope dies on navigation and activity recreate. The download does not.
     * A Download all worker owns the job it started, and cancels that job if the worker
     * itself is cancelled. A row download the pass only joined keeps running.
     */
    private suspend fun changeDownload(id: String, on: Boolean, fromBatch: Boolean) {
        val safe = requireBookId(id)
        if (!on) {
            val job = bookJobs[safe]
            job?.cancel()
            job?.join()
            withContext(Dispatchers.IO) { removeDownloadFiles(safe) }
            synchronized(offlineLock) {
                onDisk.remove(safe)
                live.value = live.value - safe
                publishOfflineLocked()
            }
            return
        }
        if (withContext(Dispatchers.IO) { hasCurrentPosts(safe) }) {
            synchronized(offlineLock) {
                onDisk.add(safe)
                if (live.value[safe] is OfflineState.Failed) {
                    live.value = live.value - safe
                }
                publishOfflineLocked()
            }
            return
        }
        // A Download all worker can be cancelled in the gap before start(). Tests block here.
        if (fromBatch) hostedBeforeClaim?.invoke()
        val claim = claimDownload(safe)
        try {
            claim.job.start()
            claim.job.join()
        } catch (err: CancellationException) {
            if (fromBatch && claim.started) {
                claim.job.cancel()
                withContext(NonCancellable) { claim.job.join() }
            }
            throw err
        }
    }

    /**
     * One live download per book. A second caller joins that job. A job that is cancelled
     * or already finished is joined, then the slot is claimed again.
     */
    private suspend fun claimDownload(id: String): DownloadClaim {
        while (true) {
            val job = scope.launch(Dispatchers.IO, start = CoroutineStart.LAZY) {
                try {
                    downloadBody(id)
                } finally {
                    bookJobs.remove(id, coroutineContext[Job])
                }
            }
            val existing = bookJobs.putIfAbsent(id, job)
            if (existing == null) return DownloadClaim(job, started = true)
            job.cancel()
            if (!existing.isCancelled && !existing.isCompleted) {
                return DownloadClaim(existing, started = false)
            }
            existing.join()
            bookJobs.remove(id, existing)
        }
    }

    override suspend fun downloadAll() {
        val current = batch
        if (current != null && !current.isCompleted) {
            cancelBatch(current)
            return
        }
        val job = scope.launch(start = CoroutineStart.LAZY) {
            try {
                publishOffline()
                val queue = ArrayDeque(
                    withContext(Dispatchers.IO) {
                        _library.value.books.map { it.id }.filter { !hasCurrentPosts(it) }
                    },
                )
                val gate = Mutex()
                coroutineScope {
                    repeat(2) {
                        launch {
                            while (isActive) {
                                val next = gate.withLock { queue.removeFirstOrNull() } ?: break
                                changeDownload(next, on = true, fromBatch = true)
                            }
                        }
                    }
                }
            } finally {
                if (batch === coroutineContext[Job]) batch = null
                publishOffline()
            }
        }
        batch = job
        job.start()
        job.join()
    }

    override suspend fun saveProgress(id: String, index: Int) {
        requireSession()
        val safe = requireBookId(id)
        outbox.enqueue(OutboxOp.Progress(safe, index.coerceAtLeast(0)))
        mutex.withLock {
            val books = _library.value.books
            publish(books, LibraryEvent.BooksReplaced(books))
        }
    }

    override suspend fun likes(id: String): Set<String> = withContext(Dispatchers.IO) {
        val session = requireSession()
        val safe = requireBookId(id)
        val base = try {
            readLikes(session, safe)
        } catch (err: IOException) {
            if (outbox.pending.value.none { it is OutboxOp.Like && it.bookId == safe }) throw err
            emptySet()
        }
        overlayLikes(base, outbox.pending.value, safe)
    }

    override suspend fun toggleLike(id: String, postId: String): Set<String> =
        withContext(Dispatchers.IO) {
            requireSession()
            val safe = requireBookId(id)
            val base = readSocial(safe)?.likes ?: emptySet()
            outbox.enqueueLike(safe, postId, base)
        }

    override suspend fun commentIndex(id: String): CommentIndex = withContext(Dispatchers.IO) {
        val session = requireSession()
        val safe = requireBookId(id)
        val index = try {
            val remote = sessions.expireOn401(session) { client.commentIndex(session, safe) }
            synchronized(socialLock) {
                if (sessions.session.value.token == session.token) {
                    writeSocial(safe) { it.copy(index = remote) }
                }
            }
            remote
        } catch (err: IOException) {
            val cached = readSocial(safe)?.index
            val pending = outbox.pending.value.any { it is OutboxOp.Reply && it.bookId == safe }
            if (cached == null && !pending) throw err
            cached ?: CommentIndex()
        }
        overlayIndex(index, outbox.pending.value, safe, readSocial(safe)?.threads.orEmpty())
    }

    override suspend fun comments(id: String, postId: String): List<PostComment> =
        withContext(Dispatchers.IO) {
            val session = requireSession()
            val safe = requireBookId(id)
            val thread = try {
                val remote = sessions.expireOn401(session) { client.comments(session, safe, postId) }
                synchronized(socialLock) {
                    if (sessions.session.value.token != session.token) return@synchronized
                    writeSocial(safe) {
                        it.copy(
                            threads = it.threads + (postId to remote),
                            index = it.index.afterThread(postId, remote),
                        )
                    }
                }
                remote
            } catch (err: IOException) {
                val cached = readSocial(safe)?.threads?.get(postId)
                val pending = outbox.pending.value.any { op ->
                    op is OutboxOp.Reply && op.bookId == safe && op.postId == postId
                }
                if (cached == null && !pending) throw IOException("You're offline.", err)
                cached.orEmpty()
            }
            overlayThread(thread, outbox.pending.value, safe, postId, session.username.orEmpty())
        }

    override suspend fun addComment(id: String, postId: String, text: String): List<PostComment> =
        withContext(Dispatchers.IO) {
            val session = requireSession()
            val safe = requireBookId(id)
            val book = book(safe) ?: error("That book is gone.")
            val trimmed = text.trim()
            if (trimmed.isEmpty()) error("Write something first.")
            val asked = trimmed.length
            if (asked > MAX_COMMENT_CHARS) {
                error("Comment is longer than $MAX_COMMENT_CHARS characters (asked $asked).")
            }
            outbox.enqueue(
                OutboxOp.Reply(
                    bookId = safe,
                    postId = postId,
                    id = UUID.randomUUID().toString(),
                    text = trimmed,
                    `private` = replyIsPrivate(book),
                    createdAt = System.currentTimeMillis(),
                ),
            )
            overlayThread(
                readSocial(safe)?.threads?.get(postId).orEmpty(),
                outbox.pending.value,
                safe,
                postId,
                session.username.orEmpty(),
            )
        }

    override fun unsentReplies(): Int = outbox.unsentReplies()

    override suspend fun retryReply(commentId: String) = outbox.retry(commentId)

    override suspend fun discardReply(commentId: String) = outbox.discard(commentId)

    override suspend fun discardOutbox() = outbox.discardAll()

    override suspend fun deleteComment(
        id: String,
        postId: String,
        commentId: String,
    ): List<PostComment> = withContext(Dispatchers.IO) {
        val session = requireSession()
        val safe = requireBookId(id)
        val remote = sessions.expireOn401(session) {
            client.deleteComment(session, safe, postId, commentId)
        }
        synchronized(socialLock) {
            if (sessions.session.value.token == session.token) {
                writeSocial(safe) { cache ->
                    cache.copy(
                        threads = cache.threads + (postId to remote),
                        index = cache.index.afterThread(postId, remote),
                    )
                }
            }
        }
        overlayThread(remote, outbox.pending.value, safe, postId, session.username.orEmpty())
    }

    override suspend fun searchPage(query: String, kind: SearchKind, cursor: String?): SearchPage =
        withContext(Dispatchers.IO) {
            if (kind == SearchKind.Catalog) error("Catalog does not use /v1/search.")
            val session = requireSession()
            sessions.expireOn401(session) { client.search(session, query, kind, cursor) }
        }

    override suspend fun homeFeed(cursor: String?): HomeFeedPage = withContext(Dispatchers.IO) {
        val session = requireSession()
        sessions.expireOn401(session) { client.homeFeed(session, cursor) }
    }

    override suspend fun profile(username: String): ProfileRecord = withContext(Dispatchers.IO) {
        val session = requireSession()
        sessions.expireOn401(session) { client.user(session, username) }
    }

    override suspend fun profilePosts(username: String, cursor: String?): ProfilePostsPage =
        withContext(Dispatchers.IO) {
            val session = requireSession()
            sessions.expireOn401(session) { client.userPosts(session, username, cursor) }
        }

    override suspend fun profileReposts(username: String, cursor: String?): ProfileRepostsPage =
        withContext(Dispatchers.IO) {
            val session = requireSession()
            sessions.expireOn401(session) { client.userReposts(session, username, cursor) }
        }

    override suspend fun profileBooks(username: String): ProfileBooks = withContext(Dispatchers.IO) {
        val session = requireSession()
        sessions.expireOn401(session) { client.userBooks(session, username) }
    }

    override suspend fun setRepost(id: String, postId: String, reposted: Boolean): RepostState =
        withContext(Dispatchers.IO) {
            val session = requireSession()
            val safe = requireBookId(id)
            sessions.expireOn401(session) { client.setRepost(session, safe, postId, reposted) }
        }

    override suspend fun search(query: String): SearchResult = withContext(Dispatchers.IO) {
        val q = query.trim()
        if (q.isEmpty()) return@withContext SearchResult()
        val books = _library.value.books
        val bookHits = books.filter { book ->
            book.title.contains(q, ignoreCase = true) ||
                book.author.contains(q, ignoreCase = true) ||
                book.handle.contains(q, ignoreCase = true)
        }
        val postHits = books.flatMap { book ->
            // Feed reads and long feeds are slow. Stop when a newer keystroke cancels us.
            ensureActive()
            val posts = cachedFeed(book.id) ?: return@flatMap emptyList()
            posts.filter { post ->
                ensureActive()
                post.text.contains(q, ignoreCase = true) ||
                    post.chapter.contains(q, ignoreCase = true)
            }.map { PostHit(book, it) }
        }
        SearchResult(books = bookHits, posts = postHits)
    }

    override suspend fun likedPosts(): List<PostHit> = withContext(Dispatchers.IO) {
        val session = sessions.session.value
        if (!session.signedIn) return@withContext emptyList()
        _library.value.books.flatMap { book ->
            val ids = overlayLikes(
                sessions.expireOn401(session) { client.likes(session, book.id) },
                outbox.pending.value,
                book.id,
            )
            if (ids.isEmpty()) emptyList()
            else {
                val posts = cachedFeed(book.id) ?: emptyList()
                posts.filter { it.id in ids }.map { PostHit(book, it) }
            }
        }
    }

    override suspend fun profileReplies(handle: String): List<ProfileReply> =
        withContext(Dispatchers.IO) {
            val session = requireSession()
            val remote = sessions.expireOn401(session) { client.profileReplies(session, handle) }
            val books = _library.value.books.associateBy { it.id }
            val feeds = mutableMapOf<String, Map<String, FeedPost>>()
            remote.replies.mapNotNull { row ->
                val book = books[row.bookId] ?: return@mapNotNull null
                val posts = feeds.getOrPut(book.id) {
                    val list = cachedFeed(book.id) ?: runCatching { feed(book.id) }.getOrNull().orEmpty()
                    list.associateBy { it.id }
                }
                val post = posts[row.postId] ?: return@mapNotNull null
                ProfileReply(book = book, post = post, comment = row.comment)
            }
        }

    private suspend fun ingest(
        session: HostedSession,
        epub: File,
        isSample: Boolean,
        scratch: File,
    ): BookRecord {
        val meta = opener.metadata(epub)
        val cover = File(scratch, "cover.jpg")
        writeCover(cover, meta.cover)
        val record = sessions.expireOn401(session) {
            client.upload(
                session = session,
                epub = epub,
                cover = cover.takeIf { it.exists() },
                title = meta.title,
                author = meta.author,
                handle = slug(meta.author),
                isSample = isSample,
            )
        }
        val safe = requireBookId(record.id)
        val dest = bookDir(safe).apply { mkdirs() }
        if (cover.exists()) {
            cover.copyTo(File(dest, "cover.jpg"), overwrite = true)
        }
        replaceBook(record.copy(id = safe))
        return record.copy(id = safe)
    }

    private fun writeCover(dest: File, cover: Bitmap?) {
        if (cover == null) return
        dest.outputStream().use { out ->
            cover.compress(Bitmap.CompressFormat.JPEG, 85, out)
        }
    }

    private fun ensureCover(session: HostedSession, id: String) {
        val dest = File(bookDir(id), "cover.jpg")
        if (dest.exists() && dest.length() > 0L) return
        sessions.expireOn401(session) { client.downloadCover(session, id, dest) }
    }

    private fun cachedFeed(id: String): List<FeedPost>? {
        val safe = safeBookId(id) ?: return null
        return readHostedPosts(File(bookDir(safe), POSTS_CACHE), json)
    }

    private suspend fun replaceBook(record: BookRecord) {
        mutex.withLock {
            epoch += 1
            if (_library.value.books.none { it.id == record.id }) {
                importedWhileRefreshing.add(record.id)
            }
            val books = (_library.value.books.filterNot { it.id == record.id } + record)
                .sortedByDescending { it.importedAt }
            publish(books, LibraryEvent.BooksReplaced(books))
        }
    }

    private suspend fun publish(
        books: List<BookRecord>,
        event: LibraryEvent,
        owner: HostedSession = sessions.session.value,
    ) {
        outbox.reconcileProgress(books)
        val snap = outbox.view()
        val shown = overlayProgress(books, snap.ops, snap.progress)
        val shownEvent = when (event) {
            is LibraryEvent.RefreshSucceeded -> LibraryEvent.RefreshSucceeded(shown)
            is LibraryEvent.BooksReplaced -> LibraryEvent.BooksReplaced(shown)
            is LibraryEvent.Cached -> LibraryEvent.Cached(shown)
            else -> event
        }
        if (sessions.session.value.token == owner.token) {
            val previous = _library.value.books
            _library.value = LibrarySnapshot(shown)
            _load.value = reduceLibraryLoad(_load.value, shownEvent)
            if (event is LibraryEvent.RefreshSucceeded) forgetPosts(previous, books)
            rescan(shown)
        }
        val url = owner.normalizedUrl()
        val user = owner.username
        if (owner.signedIn && url != null && !user.isNullOrBlank() &&
            (event is LibraryEvent.BooksReplaced || event is LibraryEvent.RefreshSucceeded)
        ) {
            withContext(Dispatchers.IO) {
                noteDisk()
                runCatching { shelfCache.write(url, user, shown) }
            }
        }
    }

    private fun readLikes(session: HostedSession, id: String): Set<String> {
        return try {
            val likes = sessions.expireOn401(session) { client.likes(session, id) }
            synchronized(socialLock) {
                if (sessions.session.value.token == session.token) {
                    writeSocial(id) { it.copy(likes = likes) }
                }
            }
            likes
        } catch (err: IOException) {
            readSocial(id)?.likes ?: throw err
        }
    }

    private suspend fun forgetPosts(previous: List<BookRecord>, next: List<BookRecord>) {
        withContext(Dispatchers.IO) {
            synchronized(socialLock) {
                noteDisk()
                val kept = next.map { it.id }.toSet()
                for (book in previous) {
                    if (book.id in kept) continue
                    val safe = safeBookId(book.id) ?: continue
                    File(bookDir(safe), POSTS_CACHE).delete()
                    File(bookDir(safe), SOCIAL_FILE).delete()
                    onDisk.remove(safe)
                    stalePosts.remove(safe)
                }
            }
        }
    }

    private fun requireSession(): HostedSession {
        val session = sessions.session.value
        if (!session.signedIn) {
            error(HOSTED_NOT_SIGNED_IN)
        }
        return session
    }

    private fun bookDir(id: String): File = File(root, id)

    private suspend fun wipeCache() {
        cancelDownloads()
        withContext(Dispatchers.IO) {
            synchronized(socialLock) {
                synchronized(offlineLock) {
                    live.value = emptyMap()
                    onDisk.clear()
                    stalePosts.clear()
                    publishOfflineLocked()
                }
                noteDisk()
                root.deleteRecursively()
                root.mkdirs()
            }
        }
    }

    private suspend fun downloadBody(id: String) = withContext(Dispatchers.IO) {
        setLive(id, OfflineState.Downloading(null))
        try {
            val session = requireSession()
            val fetched = runInterruptible {
                val posts = sessions.expireOn401(session) {
                    client.posts(session, id) { fraction ->
                        setLive(id, OfflineState.Downloading(fraction))
                    }
                }
                ensureCover(session, id)
                val likes = sessions.expireOn401(session) { client.likes(session, id) }
                val index = sessions.expireOn401(session) { client.commentIndex(session, id) }
                DownloadBundle(posts, likes, index)
            }
            ensureActive()
            if (!writeCachedPosts(id, fetched.posts, session, onShelf = true)) {
                setLive(id, null)
                return@withContext
            }
            synchronized(socialLock) {
                if (sessions.session.value.token == session.token) {
                    writeSocial(id) { it.copy(likes = fetched.likes, index = fetched.index) }
                }
            }
            setLive(id, null)
        } catch (err: CancellationException) {
            setLive(id, null)
            throw err
        } catch (err: Exception) {
            if (!isActive) {
                setLive(id, null)
                throw CancellationException("Download cancelled")
            }
            setLive(id, OfflineState.Failed(err.message ?: "Could not download."))
        }
    }

    /**
     * Writes posts.json only while [session] is still current, under the same lock as the
     * sign-out wipe. Returns false when the file was left untouched.
     */
    private suspend fun writeCachedPosts(
        id: String,
        posts: List<FeedPost>,
        session: HostedSession,
        onShelf: Boolean = false,
    ): Boolean {
        val wrote = synchronized(socialLock) {
            if (sessions.session.value.token != session.token) return@synchronized false
            if (onShelf && _library.value.books.none { it.id == id }) return@synchronized false
            writePostsFile(id, posts)
            true
        }
        if (!wrote) return false
        synchronized(offlineLock) {
            onDisk.add(id)
            if (live.value[id] is OfflineState.Failed) {
                live.value = live.value - id
            }
            publishOfflineLocked()
        }
        // The server set postCount when it parsed; mirror it so Finished works before the next load.
        book(id)?.takeIf { it.postCount != posts.size }?.let { replaceBook(it.copy(postCount = posts.size)) }
        return true
    }

    private fun writePostsFile(id: String, posts: List<FeedPost>) {
        val dir = bookDir(id).apply { mkdirs() }
        // Builds before server parsing kept the EPUB and a Readium feed here.
        File(dir, "book.epub").delete()
        File(dir, "feed.json").delete()
        File(dir, POSTS_CACHE).writeText(
            json.encodeToString(FeedCache(version = HOSTED_POSTS_CACHE_VERSION, posts = posts)),
        )
        stalePosts.remove(id)
    }

    private fun removeDownloadFiles(id: String) {
        File(bookDir(id), POSTS_CACHE).delete()
        File(bookDir(id), SOCIAL_FILE).delete()
        stalePosts.remove(id)
    }

    private fun hasCurrentPosts(id: String): Boolean {
        val safe = safeBookId(id) ?: return false
        val file = File(bookDir(safe), POSTS_CACHE)
        if (!file.isFile) {
            stalePosts.remove(safe)
            return false
        }
        if (safe in stalePosts) return false
        val posts = readHostedPosts(file, json)
        if (posts != null) {
            stalePosts.remove(safe)
            return true
        }
        stalePosts.add(safe)
        return false
    }

    private fun readSocial(id: String): SocialCache? {
        val file = File(bookDir(id), SOCIAL_FILE)
        if (!file.isFile) return null
        return runCatching {
            val cache = json.decodeFromString<SocialCache>(file.readText())
            if (cache.version == SOCIAL_CACHE_VERSION) cache else null
        }.getOrNull()
    }

    private fun writeSocial(id: String, update: (SocialCache) -> SocialCache) {
        synchronized(socialLock) {
            val next = update(readSocial(id) ?: SocialCache())
            val dir = bookDir(id).apply { mkdirs() }
            File(dir, SOCIAL_FILE).writeText(json.encodeToString(next))
        }
    }

    private fun setLive(id: String, state: OfflineState?) {
        synchronized(offlineLock) {
            live.value = if (state == null) live.value - id else live.value + (id to state)
            publishOfflineLocked()
        }
    }

    /** Drops ids that left the shelf or lost their file. Parses a posts file once, the first time it is seen. */
    private suspend fun rescan(books: List<BookRecord>) {
        val ids = books.map { it.id }.toSet()
        val (gone, present) = withContext(Dispatchers.IO) {
            noteDisk()
            stalePosts.filter { it !in ids || !File(bookDir(it), POSTS_CACHE).isFile }
                .forEach { stalePosts.remove(it) }
            val gone = onDisk.filter { it !in ids || !File(bookDir(it), POSTS_CACHE).isFile }
            val fresh = ids.filter { it !in onDisk && it !in stalePosts }
            gone to fresh.filter { hasCurrentPosts(it) }
        }
        gone.forEach { onDisk.remove(it) }
        synchronized(offlineLock) {
            present.forEach { onDisk.add(it) }
            live.value = live.value.filterKeys { it in ids }
            publishOfflineLocked()
        }
    }

    private fun publishOffline() {
        synchronized(offlineLock) { publishOfflineLocked() }
    }

    private fun publishOfflineLocked() {
        val books = _library.value.books
        val marks = live.value
        val running = batch?.isActive == true
        val map = LinkedHashMap<String, OfflineState>(books.size)
        var downloaded = 0
        for (book in books) {
            val state = when (val mark = marks[book.id]) {
                is OfflineState.Downloading, is OfflineState.Failed -> mark
                else -> if (book.id in onDisk) OfflineState.Downloaded else OfflineState.NotDownloaded
            }
            if (state is OfflineState.Downloaded) downloaded += 1
            map[book.id] = state
        }
        _offline.value = map
        _summary.value = OfflineSummary(downloaded, books.size, running)
    }

    private suspend fun cancelDownloads() {
        val running = batch
        val jobs = bookJobs.values.toList()
        val feeds = feedJobs.toList()
        running?.cancel()
        jobs.forEach { it.cancel() }
        feeds.forEach { it.cancel() }
        running?.join()
        jobs.forEach { it.join() }
        feeds.forEach { it.join() }
    }

    /** Stops a Download all pass. Each worker cancels and joins the book job it started. */
    private suspend fun cancelBatch(running: Job) {
        running.cancel()
        running.join()
    }

    private suspend fun clearSocialFiles() {
        withContext(Dispatchers.IO) {
            synchronized(socialLock) {
                noteDisk()
                root.listFiles()?.forEach { dir ->
                    if (dir.isDirectory) File(dir, SOCIAL_FILE).delete()
                }
            }
        }
    }
}

private data class DownloadClaim(val job: Job, val started: Boolean)

private data class DownloadBundle(
    val posts: List<FeedPost>,
    val likes: Set<String>,
    val index: CommentIndex,
)

fun safeBookId(id: String): String? {
    return runCatching { UUID.fromString(id).toString() }.getOrNull()
}

fun requireBookId(id: String): String {
    return safeBookId(id) ?: error("Bad book id.")
}

const val HOSTED_NOT_SIGNED_IN = "Sign in to the hosted library first."

/** Server posts cached per hosted book. Bump the version if the server's post shape changes. */
private const val POSTS_CACHE = "posts.json"
private const val SOCIAL_FILE = "social.json"
private const val HOSTED_POSTS_CACHE_VERSION = 1

/** Decode attempts of a posts file that exists. Tests use this to catch repeat scans. */
internal val hostedPostsDecodes = AtomicInteger(0)

/** Threads that touched the hosted shelf files. Tests use this to catch main-thread IO. */
internal val hostedDiskThreads = ConcurrentLinkedQueue<String>()

/** When set, a Download all worker blocks here before it claims a book. Tests cancel in that gap. */
internal var hostedBeforeClaim: (() -> Unit)? = null

internal fun noteDisk() {
    hostedDiskThreads.add(Thread.currentThread().name)
}

/** A hit means [HostedLibraryStore.feed] can skip the network. A version mismatch is a miss. */
internal fun readHostedPosts(file: File, json: Json): List<FeedPost>? {
    if (!file.exists()) return null
    hostedPostsDecodes.incrementAndGet()
    return runCatching {
        val cache = json.decodeFromString<FeedCache>(file.readText())
        if (cache.version == HOSTED_POSTS_CACHE_VERSION) cache.posts else null
    }.getOrNull()
}
