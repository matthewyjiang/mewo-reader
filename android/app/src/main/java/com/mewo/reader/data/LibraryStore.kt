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
 * will talk to a server. Same calls either way. EPUB parse stays on
 * the phone; a hosted adapter downloads the file, then extracts here.
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

    suspend fun search(query: String): SearchResult

    suspend fun likedPosts(): List<PostHit>

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
            kind.collect { which -> active(which).load() }
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

    override suspend fun search(query: String) = active().search(query)

    override suspend fun likedPosts() = active().likedPosts()

    override fun coverFile(id: String) = active().coverFile(id)
}
