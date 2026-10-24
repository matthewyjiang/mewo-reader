package com.mewo.reader.data

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.isActive
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.supervisorScope
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import java.io.File
import java.io.IOException
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

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
    scope: CoroutineScope,
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
    private val socialLock = Any()

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

    init {
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
                    cancelDownloads()
                    clearShelf(previous)
                    if (signedOut || urlChanged) wipeCache() else clearSocialFiles()
                    mutex.withLock {
                        _library.value = LibrarySnapshot()
                        _load.value = if (session.signedIn) LibraryLoading else LibraryLoaded(emptyList(), false)
                    }
                    live.value = emptyMap()
                    onDisk.clear()
                    publishOffline()
                }
                previous = session
                launch { runCatching { load() } }
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
        val start = mutex.withLock {
            if (sessions.session.value.token != session.token) return@withLock null
            if (cached != null && _library.value.books.isEmpty()) {
                _library.value = LibrarySnapshot(cached)
                _load.value = reduceLibraryLoad(_load.value, LibraryEvent.Cached(cached))
            }
            _load.value = reduceLibraryLoad(_load.value, LibraryEvent.RefreshStarted)
            importedWhileRefreshing.clear()
            epoch
        } ?: return
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

    private fun clearShelf(session: HostedSession) {
        val url = session.normalizedUrl() ?: return
        val user = session.username ?: return
        shelfCache.clear(url, user)
        FileHomeFeedCache(File(context.filesDir, "home-feeds")).clear("$url\n$user")
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
        onDisk.remove(safe)
        setLive(safe, null)
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
        cachedFeed(safe)?.let { return@withContext it }
        val posts = sessions.expireOn401(session) { client.posts(session, safe) }
        writeCachedPosts(safe, posts)
        posts
    }

    override suspend fun setDownloaded(id: String, on: Boolean) {
        val safe = requireBookId(id)
        if (!on) {
            val job = bookJobs[safe]
            job?.cancel()
            job?.join()
            withContext(Dispatchers.IO) { removeDownloadFiles(safe) }
            setLive(safe, null)
            return
        }
        if (withContext(Dispatchers.IO) { hasCurrentPosts(safe) }) {
            onDisk.add(safe)
            publishOffline()
            return
        }
        bookJobs[safe]?.let { existing ->
            if (existing.isActive) {
                existing.join()
                return
            }
        }
        supervisorScope {
            val job = launch(Dispatchers.IO) { downloadBody(safe) }
            bookJobs[safe] = job
            try {
                job.join()
            } catch (err: CancellationException) {
                if (!isActive) throw err
            } finally {
                bookJobs.remove(safe, job)
            }
        }
    }

    override suspend fun downloadAll() {
        val current = batch
        if (current?.isActive == true) {
            current.cancel()
            current.join()
            return
        }
        coroutineScope {
            batch = coroutineContext[Job]
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
                                setDownloaded(next, true)
                            }
                        }
                    }
                }
            } finally {
                if (batch === coroutineContext[Job]) batch = null
                publishOffline()
            }
        }
    }

    override suspend fun saveProgress(id: String, index: Int) {
        val session = requireSession()
        val safe = requireBookId(id)
        val updated = withContext(Dispatchers.IO) {
            sessions.expireOn401(session) {
                client.patch(session, safe, PatchBook(progressIndex = index.coerceAtLeast(0)))
            }
        }
        replaceBook(updated)
    }

    override suspend fun likes(id: String): Set<String> = withContext(Dispatchers.IO) {
        val session = requireSession()
        val safe = requireBookId(id)
        val likes = try {
            sessions.expireOn401(session) { client.likes(session, safe) }
        } catch (err: IOException) {
            return@withContext readSocial(safe)?.likes ?: throw err
        }
        writeSocial(safe) { it.copy(likes = likes) }
        likes
    }

    override suspend fun toggleLike(id: String, postId: String): Set<String> =
        withContext(Dispatchers.IO) {
            val session = requireSession()
            val safe = requireBookId(id)
            sessions.expireOn401(session) { client.toggleLike(session, safe, postId) }
        }

    override suspend fun commentIndex(id: String): CommentIndex = withContext(Dispatchers.IO) {
        val session = requireSession()
        val safe = requireBookId(id)
        val index = try {
            sessions.expireOn401(session) { client.commentIndex(session, safe) }
        } catch (err: IOException) {
            return@withContext readSocial(safe)?.index ?: throw err
        }
        writeSocial(safe) { it.copy(index = index) }
        index
    }

    override suspend fun comments(id: String, postId: String): List<PostComment> =
        withContext(Dispatchers.IO) {
            val session = requireSession()
            val safe = requireBookId(id)
            val thread = try {
                sessions.expireOn401(session) { client.comments(session, safe, postId) }
            } catch (err: IOException) {
                return@withContext readSocial(safe)?.threads?.get(postId)
                    ?: throw IOException("You're offline.", err)
            }
            writeSocial(safe) {
                it.copy(
                    threads = it.threads + (postId to thread),
                    index = it.index.afterThread(postId, thread),
                )
            }
            thread
        }

    override suspend fun addComment(id: String, postId: String, text: String): List<PostComment> =
        withContext(Dispatchers.IO) {
            val session = requireSession()
            val safe = requireBookId(id)
            sessions.expireOn401(session) { client.addComment(session, safe, postId, text) }
        }

    override suspend fun deleteComment(
        id: String,
        postId: String,
        commentId: String,
    ): List<PostComment> = withContext(Dispatchers.IO) {
        val session = requireSession()
        val safe = requireBookId(id)
        sessions.expireOn401(session) { client.deleteComment(session, safe, postId, commentId) }
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
            val ids = sessions.expireOn401(session) { client.likes(session, book.id) }
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

    private fun publish(
        books: List<BookRecord>,
        event: LibraryEvent,
        owner: HostedSession = sessions.session.value,
    ) {
        if (sessions.session.value.token == owner.token) {
            val previous = _library.value.books
            _library.value = LibrarySnapshot(books)
            _load.value = reduceLibraryLoad(_load.value, event)
            if (event is LibraryEvent.RefreshSucceeded) forgetPosts(previous, books)
            rescan(books)
            publishOffline()
        }
        val url = owner.normalizedUrl()
        val user = owner.username
        if (owner.signedIn && url != null && !user.isNullOrBlank() &&
            (event is LibraryEvent.BooksReplaced || event is LibraryEvent.RefreshSucceeded)
        ) {
            runCatching { shelfCache.write(url, user, books) }
        }
    }

    private fun forgetPosts(previous: List<BookRecord>, next: List<BookRecord>) {
        val kept = next.map { it.id }.toSet()
        for (book in previous) {
            if (book.id in kept) continue
            val safe = safeBookId(book.id) ?: continue
            File(bookDir(safe), POSTS_CACHE).delete()
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

    private fun wipeCache() {
        cancelDownloads()
        live.value = emptyMap()
        onDisk.clear()
        root.deleteRecursively()
        root.mkdirs()
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
            if (sessions.session.value.token != session.token || _library.value.books.none { it.id == id }) {
                setLive(id, null)
                return@withContext
            }
            writeCachedPosts(id, fetched.posts)
            writeSocial(id) { it.copy(likes = fetched.likes, index = fetched.index) }
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

    private suspend fun writeCachedPosts(id: String, posts: List<FeedPost>) {
        val dir = bookDir(id).apply { mkdirs() }
        // Builds before server parsing kept the EPUB and a Readium feed here.
        File(dir, "book.epub").delete()
        File(dir, "feed.json").delete()
        File(dir, POSTS_CACHE).writeText(
            json.encodeToString(FeedCache(version = HOSTED_POSTS_CACHE_VERSION, posts = posts)),
        )
        onDisk.add(id)
        // The server set postCount when it parsed; mirror it so Finished works before the next load.
        book(id)?.takeIf { it.postCount != posts.size }?.let { replaceBook(it.copy(postCount = posts.size)) }
        publishOffline()
    }

    private fun removeDownloadFiles(id: String) {
        File(bookDir(id), POSTS_CACHE).delete()
        File(bookDir(id), SOCIAL_FILE).delete()
        onDisk.remove(id)
    }

    private fun hasCurrentPosts(id: String): Boolean {
        val safe = safeBookId(id) ?: return false
        return readHostedPosts(File(bookDir(safe), POSTS_CACHE), json) != null
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
        live.update { current ->
            if (state == null) current - id else current + (id to state)
        }
        publishOffline()
    }

    private fun rescan(books: List<BookRecord>) {
        val ids = books.map { it.id }.toSet()
        onDisk.removeIf { it !in ids || !hasCurrentPosts(it) }
        for (id in ids) {
            if (hasCurrentPosts(id)) onDisk.add(id)
        }
        live.update { current -> current.filterKeys { it in ids } }
    }

    private fun publishOffline() {
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

    private fun cancelDownloads() {
        batch?.cancel()
        bookJobs.values.toList().forEach { it.cancel() }
    }

    private fun clearSocialFiles() {
        root.listFiles()?.forEach { dir ->
            if (dir.isDirectory) File(dir, SOCIAL_FILE).delete()
        }
    }
}

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

/** A hit means [HostedLibraryStore.feed] can skip the network. A version mismatch is a miss. */
internal fun readHostedPosts(file: File, json: Json): List<FeedPost>? {
    if (!file.exists()) return null
    return runCatching {
        val cache = json.decodeFromString<FeedCache>(file.readText())
        if (cache.version == HOSTED_POSTS_CACHE_VERSION) cache.posts else null
    }.getOrNull()
}
