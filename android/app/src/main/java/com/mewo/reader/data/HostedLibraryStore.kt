package com.mewo.reader.data

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import java.io.File
import java.util.UUID

/**
 * Hosted shelf. Talks HTTP, then parses the EPUB on the phone.
 * Cache lives under filesDir/hosted so it never mixes with the local shelf.
 */
class HostedLibraryStore(
    private val context: Context,
    private val opener: EpubOpener,
    private val sessions: HostedSessionStore,
    private val client: HostedClient,
    scope: CoroutineScope,
    private val extractor: FeedExtractor = FeedExtractor(),
) : LibraryStore {
    private val json = Json {
        ignoreUnknownKeys = true
        prettyPrint = true
    }
    private val root = File(context.filesDir, "hosted").apply { mkdirs() }
    private val mutex = Mutex()
    private var epoch = 0
    private val _library = MutableStateFlow(LibrarySnapshot())
    override val library: StateFlow<LibrarySnapshot> = _library.asStateFlow()

    init {
        scope.launch {
            var wasSignedIn = sessions.session.value.signedIn
            sessions.session.collect { session ->
                if (wasSignedIn && !session.signedIn) {
                    wipeCache()
                }
                wasSignedIn = session.signedIn
                runCatching { load() }
            }
        }
    }

    override suspend fun load() {
        val session = sessions.session.value
        if (!session.signedIn) {
            mutex.withLock {
                _library.value = LibrarySnapshot()
            }
            return
        }
        val start = mutex.withLock { epoch }
        val snap = withContext(Dispatchers.IO) {
            try {
                val remote = client.library(session)
                remote.books.forEach { book ->
                    val id = safeBookId(book.id) ?: return@forEach
                    ensureCover(session, id)
                }
                remote.copy(books = remote.books.filter { safeBookId(it.id) != null })
            } catch (err: HostedException) {
                if (err.code == 401) sessions.signOut()
                throw err
            }
        }
        mutex.withLock {
            if (epoch == start) {
                _library.value = snap
            } else {
                val extras = _library.value.books.filter { local ->
                    snap.books.none { it.id == local.id }
                }
                _library.value = snap.copy(
                    books = (snap.books + extras).sortedByDescending { it.importedAt },
                )
            }
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
        withContext(Dispatchers.IO) {
            expireOn401 { client.delete(session, safe) }
            bookDir(safe).deleteRecursively()
        }
        mutex.withLock {
            epoch += 1
            _library.value = _library.value.copy(
                books = _library.value.books.filterNot { it.id == safe },
            )
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
        val epub = ensureEpub(session, safe)
        val cacheFile = File(bookDir(safe), "feed.json")
        if (cacheFile.exists()) {
            val cache = json.decodeFromString<FeedCache>(cacheFile.readText())
            if (cache.version == FEED_CACHE_VERSION) {
                return@withContext cache.posts
            }
        }
        val posts = opener.open(epub) { publication ->
            extractor.extract(publication)
        }
        bookDir(safe).mkdirs()
        cacheFile.writeText(json.encodeToString(FeedCache(version = FEED_CACHE_VERSION, posts = posts)))
        val updated = expireOn401 { client.patch(session, safe, PatchBook(postCount = posts.size)) }
        replaceBook(updated)
        posts
    }

    override suspend fun saveProgress(id: String, index: Int) {
        val session = requireSession()
        val safe = requireBookId(id)
        val updated = withContext(Dispatchers.IO) {
            expireOn401 {
                client.patch(session, safe, PatchBook(progressIndex = index.coerceAtLeast(0)))
            }
        }
        replaceBook(updated)
    }

    override suspend fun likes(id: String): Set<String> = withContext(Dispatchers.IO) {
        val safe = requireBookId(id)
        expireOn401 { client.likes(requireSession(), safe) }
    }

    override suspend fun toggleLike(id: String, postId: String): Set<String> =
        withContext(Dispatchers.IO) {
            val safe = requireBookId(id)
            expireOn401 { client.toggleLike(requireSession(), safe, postId) }
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
            val posts = cachedFeed(book.id) ?: return@flatMap emptyList()
            posts.filter { post ->
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
            val ids = expireOn401 { client.likes(session, book.id) }
            if (ids.isEmpty()) emptyList()
            else {
                val posts = cachedFeed(book.id) ?: emptyList()
                posts.filter { it.id in ids }.map { PostHit(book, it) }
            }
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
        val record = expireOn401 {
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
        epub.copyTo(File(dest, "book.epub"), overwrite = true)
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

    private fun ensureEpub(session: HostedSession, id: String): File {
        val dest = File(bookDir(id), "book.epub")
        if (!dest.exists() || dest.length() == 0L) {
            expireOn401 { client.downloadEpub(session, id, dest) }
        }
        return dest
    }

    private fun ensureCover(session: HostedSession, id: String) {
        val dest = File(bookDir(id), "cover.jpg")
        if (dest.exists() && dest.length() > 0L) return
        expireOn401 { client.downloadCover(session, id, dest) }
    }

    private fun cachedFeed(id: String): List<FeedPost>? {
        val safe = safeBookId(id) ?: return null
        val cacheFile = File(bookDir(safe), "feed.json")
        if (!cacheFile.exists()) return null
        return runCatching {
            val cache = json.decodeFromString<FeedCache>(cacheFile.readText())
            if (cache.version == FEED_CACHE_VERSION) cache.posts else null
        }.getOrNull()
    }

    private suspend fun replaceBook(record: BookRecord) {
        mutex.withLock {
            epoch += 1
            val books = _library.value.books.filterNot { it.id == record.id } + record
            _library.value = _library.value.copy(
                books = books.sortedByDescending { it.importedAt },
            )
        }
    }

    private fun requireSession(): HostedSession {
        val session = sessions.session.value
        if (!session.signedIn) {
            error(HOSTED_NOT_SIGNED_IN)
        }
        return session
    }

    private fun <T> expireOn401(block: () -> T): T {
        return try {
            block()
        } catch (err: HostedException) {
            if (err.code == 401) sessions.signOut()
            throw err
        }
    }

    private fun bookDir(id: String): File = File(root, id)

    private fun wipeCache() {
        root.deleteRecursively()
        root.mkdirs()
    }
}

fun safeBookId(id: String): String? {
    return runCatching { UUID.fromString(id).toString() }.getOrNull()
}

fun requireBookId(id: String): String {
    return safeBookId(id) ?: error("Bad book id.")
}

const val HOSTED_NOT_SIGNED_IN = "Sign in to the hosted library first."
