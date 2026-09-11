package com.mewo.reader.data

import android.net.Uri
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import java.io.File

/**
 * Library verbs the UI uses. Local keeps books on the device. Hosted
 * talks to a shared server shelf. Same calls either way. EPUB parse
 * stays on the phone; a hosted adapter downloads the file, then extracts here.
 */
interface LibraryStore {
    val library: StateFlow<LibrarySnapshot>

    suspend fun load()

    suspend fun importFromUri(uri: Uri): BookRecord

    suspend fun importSample(): BookRecord

    suspend fun delete(id: String)

    suspend fun book(id: String): BookRecord?

    suspend fun feed(id: String): List<FeedPost>

    suspend fun saveProgress(id: String, index: Int)

    suspend fun likes(id: String): Set<String>

    suspend fun toggleLike(id: String, postId: String): Set<String>

    suspend fun commentIndex(id: String): CommentIndex

    suspend fun comments(id: String, postId: String): List<PostComment>

    suspend fun addComment(id: String, postId: String, text: String): List<PostComment>

    suspend fun deleteComment(id: String, postId: String, commentId: String): List<PostComment>

    suspend fun search(query: String): SearchResult

    suspend fun likedPosts(): List<PostHit>

    /** Replies by this handle, newest first, each with the line they answered. */
    suspend fun profileReplies(handle: String): List<ProfileReply>

    /** Local path the avatar can decode. Hosted writes a cache file first. */
    fun coverFile(id: String): File?
}

/** Forwards each call to the backend chosen in settings. */
@OptIn(ExperimentalCoroutinesApi::class)
class SwitchingLibraryStore(
    private val local: LibraryStore,
    private val hosted: LibraryStore,
    private val kind: StateFlow<BackendKind>,
    scope: CoroutineScope,
) : LibraryStore {
    private fun active(which: BackendKind = kind.value): LibraryStore = when (which) {
        BackendKind.Local -> local
        BackendKind.Hosted -> hosted
    }

    override val library: StateFlow<LibrarySnapshot> = kind
        .flatMapLatest { which -> active(which).library }
        .stateIn(scope, SharingStarted.Eagerly, LibrarySnapshot())

    init {
        scope.launch {
            kind.collect { which ->
                runCatching { active(which).load() }
            }
        }
    }

    override suspend fun load() = active().load()

    override suspend fun importFromUri(uri: Uri) = active().importFromUri(uri)

    override suspend fun importSample() = active().importSample()

    override suspend fun delete(id: String) = active().delete(id)

    override suspend fun book(id: String) = active().book(id)

    override suspend fun feed(id: String) = active().feed(id)

    override suspend fun saveProgress(id: String, index: Int) = active().saveProgress(id, index)

    override suspend fun likes(id: String) = active().likes(id)

    override suspend fun toggleLike(id: String, postId: String) = active().toggleLike(id, postId)

    override suspend fun commentIndex(id: String) = active().commentIndex(id)

    override suspend fun comments(id: String, postId: String) = active().comments(id, postId)

    override suspend fun addComment(id: String, postId: String, text: String) =
        active().addComment(id, postId, text)

    override suspend fun deleteComment(id: String, postId: String, commentId: String) =
        active().deleteComment(id, postId, commentId)

    override suspend fun search(query: String) = active().search(query)

    override suspend fun likedPosts() = active().likedPosts()

    override suspend fun profileReplies(handle: String) = active().profileReplies(handle)

    override fun coverFile(id: String) = active().coverFile(id)
}
