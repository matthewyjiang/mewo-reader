package com.mewo.reader.data

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import java.io.File
import java.util.UUID

/** On-device library: JSON metadata, EPUB files, and the feed cache. */
class LocalLibraryStore(
    private val context: Context,
    private val opener: EpubOpener,
    private val profile: LocalProfileStore,
    private val extractor: FeedExtractor = FeedExtractor(),
) : LibraryStore {
    private val json = Json {
        ignoreUnknownKeys = true
        prettyPrint = true
    }
    private val root = File(context.filesDir, "books").apply { mkdirs() }
    private val libraryFile = File(root, "library.json")
    private val mutex = Mutex()

    private val _library = MutableStateFlow(LibrarySnapshot())
    override val library: StateFlow<LibrarySnapshot> = _library.asStateFlow()

    override suspend fun load() {
        mutex.withLock {
            _library.value = readLibrary()
        }
    }

    override suspend fun importFromUri(uri: Uri): BookRecord = withContext(Dispatchers.IO) {
        val id = UUID.randomUUID().toString()
        val dir = File(root, id).apply { mkdirs() }
        val dest = File(dir, "book.epub")
        context.contentResolver.openInputStream(uri)?.use { input ->
            dest.outputStream().use { input.copyTo(it) }
        } ?: error("Could not read that file")
        ingest(id = id, file = dest, isSample = false)
    }

    override suspend fun importSample(): BookRecord = withContext(Dispatchers.IO) {
        val existing = _library.value.books.firstOrNull { it.isSample }
        if (existing != null) return@withContext existing
        val id = "sample"
        val dir = File(root, id).apply { mkdirs() }
        val dest = File(dir, "book.epub")
        context.assets.open("sample.epub").use { input ->
            dest.outputStream().use { input.copyTo(it) }
        }
        ingest(id = id, file = dest, isSample = true)
    }

    override suspend fun delete(id: String) {
        mutex.withLock {
            File(root, id).deleteRecursively()
            writeLibrary(_library.value.copy(books = _library.value.books.filterNot { it.id == id }))
        }
    }

    override suspend fun book(id: String): BookRecord? = _library.value.books.find { it.id == id }

    private fun epubFile(id: String): File = File(root, "$id/book.epub")

    override fun coverFile(id: String): File = File(root, "$id/cover.jpg")

    override suspend fun feed(id: String): List<FeedPost> = withContext(Dispatchers.IO) {
        val cacheFile = File(root, "$id/feed.json")
        if (cacheFile.exists()) {
            val cache = json.decodeFromString<FeedCache>(cacheFile.readText())
            if (cache.version == FEED_CACHE_VERSION) {
                return@withContext cache.posts
            }
        }
        val posts = opener.open(epubFile(id)) { publication ->
            extractor.extract(publication)
        }
        cacheFile.writeText(json.encodeToString(FeedCache(version = FEED_CACHE_VERSION, posts = posts)))
        mutex.withLock {
            val updated = _library.value.books.map { book ->
                if (book.id == id) {
                    book.copy(postCount = posts.size, progressIndex = 0)
                } else {
                    book
                }
            }
            writeLibrary(_library.value.copy(books = updated))
        }
        posts
    }

    override suspend fun saveProgress(id: String, index: Int) {
        mutex.withLock {
            val updated = _library.value.books.map { book ->
                if (book.id == id) book.copy(progressIndex = index.coerceAtLeast(0)) else book
            }
            writeLibrary(_library.value.copy(books = updated))
        }
    }

    override suspend fun likes(id: String): Set<String> = withContext(Dispatchers.IO) {
        val file = File(root, "$id/likes.json")
        if (!file.exists()) emptySet()
        else json.decodeFromString<LikeSet>(file.readText()).ids
    }

    override suspend fun toggleLike(id: String, postId: String): Set<String> = withContext(Dispatchers.IO) {
        val file = File(root, "$id/likes.json")
        val current = likes(id).toMutableSet()
        if (!current.add(postId)) current.remove(postId)
        file.writeText(json.encodeToString(LikeSet(current)))
        current
    }

    override suspend fun commentIndex(id: String): CommentIndex = withContext(Dispatchers.IO) {
        val file = readComments(id)
        CommentIndex(
            counts = file.posts.mapValues { it.value.size },
            mine = file.posts.filterValues { it.isNotEmpty() }.keys,
        )
    }

    override suspend fun comments(id: String, postId: String): List<PostComment> =
        withContext(Dispatchers.IO) {
            localThread(readComments(id).posts[postId].orEmpty())
        }

    override suspend fun addComment(id: String, postId: String, text: String): List<PostComment> =
        withContext(Dispatchers.IO) {
            val trimmed = text.trim()
            if (trimmed.isEmpty()) error("Write something first.")
            val asked = trimmed.length
            if (asked > MAX_COMMENT_CHARS) {
                error("Comment is longer than $MAX_COMMENT_CHARS characters (asked $asked).")
            }
            val file = File(root, "$id/comments.json")
            val current = readComments(id)
            val next = current.posts.toMutableMap()
            val row = LocalComment(
                id = UUID.randomUUID().toString(),
                text = trimmed,
                createdAt = System.currentTimeMillis(),
            )
            next[postId] = next[postId].orEmpty() + row
            file.writeText(json.encodeToString(LocalCommentFile(next)))
            localThread(next[postId].orEmpty())
        }

    override suspend fun deleteComment(
        id: String,
        postId: String,
        commentId: String,
    ): List<PostComment> = withContext(Dispatchers.IO) {
        val file = File(root, "$id/comments.json")
        val current = readComments(id)
        val next = current.posts.toMutableMap()
        next[postId] = next[postId].orEmpty().filterNot { it.id == commentId }
        if (next[postId].isNullOrEmpty()) next.remove(postId)
        file.writeText(json.encodeToString(LocalCommentFile(next)))
        localThread(next[postId].orEmpty())
    }

    private fun readComments(id: String): LocalCommentFile {
        val file = File(root, "$id/comments.json")
        if (!file.exists()) return LocalCommentFile()
        return runCatching {
            json.decodeFromString<LocalCommentFile>(file.readText())
        }.getOrDefault(LocalCommentFile())
    }

    private fun localThread(rows: List<LocalComment>): List<PostComment> {
        return rows.map { row ->
            PostComment(
                id = row.id,
                username = LOCAL_COMMENT_NAME,
                text = row.text,
                createdAt = row.createdAt,
                mine = true,
            )
        }
    }

    /**
     * Title, author, handle, plus post text from books already opened.
     * Unopened books have no feed cache, so they only match on metadata.
     */
    override suspend fun search(query: String): SearchResult = withContext(Dispatchers.IO) {
        val q = query.trim()
        if (q.isEmpty()) return@withContext SearchResult()
        if (_library.value.books.isEmpty()) {
            mutex.withLock { _library.value = readLibrary() }
        }
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
        if (_library.value.books.isEmpty()) {
            mutex.withLock { _library.value = readLibrary() }
        }
        _library.value.books.flatMap { book ->
            val ids = likes(book.id)
            if (ids.isEmpty()) emptyList()
            else {
                val posts = cachedFeed(book.id) ?: emptyList()
                posts.filter { it.id in ids }.map { PostHit(book, it) }
            }
        }
    }

    override suspend fun profileReplies(handle: String): List<ProfileReply> =
        withContext(Dispatchers.IO) {
            val self = profile.profile.value.handle
            if (self.isBlank() || !handle.equals(self, ignoreCase = true)) {
                return@withContext emptyList()
            }
            if (_library.value.books.isEmpty()) {
                mutex.withLock { _library.value = readLibrary() }
            }
            _library.value.books.flatMap { book ->
                val file = readComments(book.id)
                if (file.posts.isEmpty()) return@flatMap emptyList()
                val posts = postMap(book.id)
                file.posts.flatMap { (postId, rows) ->
                    val post = posts[postId] ?: return@flatMap emptyList()
                    localThread(rows).map { comment ->
                        ProfileReply(book = book, post = post, comment = comment)
                    }
                }
            }.sortedByDescending { it.comment.createdAt }
        }

    private suspend fun postMap(id: String): Map<String, FeedPost> {
        val posts = cachedFeed(id) ?: runCatching { feed(id) }.getOrNull().orEmpty()
        return posts.associateBy { it.id }
    }

    private fun cachedFeed(id: String): List<FeedPost>? {
        val cacheFile = File(root, "$id/feed.json")
        if (!cacheFile.exists()) return null
        return runCatching {
            val cache = json.decodeFromString<FeedCache>(cacheFile.readText())
            if (cache.version == FEED_CACHE_VERSION) cache.posts else null
        }.getOrNull()
    }

    private suspend fun ingest(id: String, file: File, isSample: Boolean): BookRecord {
        val meta = opener.metadata(file)
        writeCover(id, meta.cover)
        val record = BookRecord(
            id = id,
            title = meta.title,
            author = meta.author,
            handle = slug(meta.author),
            importedAt = System.currentTimeMillis(),
            isSample = isSample,
        )
        mutex.withLock {
            val books = _library.value.books.filterNot { it.id == id } + record
            writeLibrary(LibrarySnapshot(books.sortedByDescending { it.importedAt }))
        }
        return record
    }

    private fun writeCover(id: String, cover: Bitmap?) {
        if (cover == null) return
        coverFile(id).outputStream().use { out ->
            cover.compress(Bitmap.CompressFormat.JPEG, 85, out)
        }
    }

    private fun readLibrary(): LibrarySnapshot {
        if (!libraryFile.exists()) return LibrarySnapshot()
        return runCatching {
            json.decodeFromString<LibrarySnapshot>(libraryFile.readText())
        }.getOrDefault(LibrarySnapshot())
    }

    private fun writeLibrary(snapshot: LibrarySnapshot) {
        libraryFile.writeText(json.encodeToString(snapshot))
        _library.update { snapshot }
    }
}

/** Bump when extract output changes so an already-opened book rebuilds its feed. */
const val FEED_CACHE_VERSION = 4

/** Same budget as the hosted API. See server/src/models.rs. */
const val MAX_COMMENT_CHARS = 2000

const val LOCAL_COMMENT_NAME = "You"

fun slug(value: String): String {
    val cleaned = value.lowercase()
        .replace(Regex("[^a-z0-9]+"), "")
        .take(18)
    return cleaned.ifBlank { "author" }
}
