package com.mewo.reader.data

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
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
    private val _library = MutableStateFlow(LibrarySnapshot())
    override val library: StateFlow<LibrarySnapshot> = _library.asStateFlow()

    init {
        scope.launch {
            sessions.session.collect { load() }
        }
    }

    override suspend fun load() {
        val session = sessions.session.value
        if (!session.signedIn) {
            _library.value = LibrarySnapshot()
            return
        }
        runCatching {
            val snap = withContext(Dispatchers.IO) {
                val remote = client.library(session)
                remote.books.forEach { book ->
                    ensureCover(session, book.id)
                }
                remote
            }
            mutex.withLock {
                _library.value = snap
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
        withContext(Dispatchers.IO) {
            client.delete(session, id)
            File(root, id).deleteRecursively()
        }
        mutex.withLock {
            _library.update { snap ->
                snap.copy(books = snap.books.filterNot { it.id == id })
            }
        }
    }

    override suspend fun book(id: String): BookRecord? = _library.value.books.find { it.id == id }

    override fun coverFile(id: String): File = File(root, "$id/cover.jpg")

    override suspend fun feed(id: String): List<FeedPost> = withContext(Dispatchers.IO) {
        val session = requireSession()
        val epub = ensureEpub(session, id)
        val cacheFile = File(root, "$id/feed.json")
        if (cacheFile.exists()) {
            val cache = json.decodeFromString<FeedCache>(cacheFile.readText())
            if (cache.version == FEED_CACHE_VERSION) {
                return@withContext cache.posts
            }
        }
        val posts = opener.open(epub) { publication ->
            extractor.extract(publication)
        }
        File(root, id).mkdirs()
        cacheFile.writeText(json.encodeToString(FeedCache(version = FEED_CACHE_VERSION, posts = posts)))
        val updated = client.patch(session, id, PatchBook(postCount = posts.size))
        replaceBook(updated)
        posts
    }

    override suspend fun saveProgress(id: String, index: Int) {
        val session = requireSession()
        val updated = withContext(Dispatchers.IO) {
            client.patch(session, id, PatchBook(progressIndex = index.coerceAtLeast(0)))
        }
        replaceBook(updated)
    }

    override suspend fun likes(id: String): Set<String> = withContext(Dispatchers.IO) {
        client.likes(requireSession(), id)
    }

    override suspend fun toggleLike(id: String, postId: String): Set<String> =
        withContext(Dispatchers.IO) {
            client.toggleLike(requireSession(), id, postId)
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
        val session = requireSession()
        _library.value.books.flatMap { book ->
            val ids = client.likes(session, book.id)
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
        val record = client.upload(
            session = session,
            epub = epub,
            cover = cover.takeIf { it.exists() },
            title = meta.title,
            author = meta.author,
            handle = slug(meta.author),
            isSample = isSample,
        )
        val dest = File(root, record.id).apply { mkdirs() }
        epub.copyTo(File(dest, "book.epub"), overwrite = true)
        if (cover.exists()) {
            cover.copyTo(File(dest, "cover.jpg"), overwrite = true)
        }
        replaceBook(record)
        return record
    }

    private fun writeCover(dest: File, cover: Bitmap?) {
        if (cover == null) return
        dest.outputStream().use { out ->
            cover.compress(Bitmap.CompressFormat.JPEG, 85, out)
        }
    }

    private fun ensureEpub(session: HostedSession, id: String): File {
        val dest = File(root, "$id/book.epub")
        if (!dest.exists() || dest.length() == 0L) {
            client.downloadEpub(session, id, dest)
        }
        return dest
    }

    private fun ensureCover(session: HostedSession, id: String) {
        val dest = coverFile(id)
        if (dest.exists() && dest.length() > 0L) return
        client.downloadCover(session, id, dest)
    }

    private fun cachedFeed(id: String): List<FeedPost>? {
        val cacheFile = File(root, "$id/feed.json")
        if (!cacheFile.exists()) return null
        return runCatching {
            val cache = json.decodeFromString<FeedCache>(cacheFile.readText())
            if (cache.version == FEED_CACHE_VERSION) cache.posts else null
        }.getOrNull()
    }

    private fun replaceBook(record: BookRecord) {
        _library.update { snap ->
            val books = snap.books.filterNot { it.id == record.id } + record
            snap.copy(books = books.sortedByDescending { it.importedAt })
        }
    }

    private fun requireSession(): HostedSession {
        val session = sessions.session.value
        if (!session.signedIn) {
            error(HOSTED_NOT_SIGNED_IN)
        }
        return session
    }
}

const val HOSTED_NOT_SIGNED_IN = "Sign in to the hosted library first."
const val HOSTED_NOT_CONNECTED = HOSTED_NOT_SIGNED_IN
