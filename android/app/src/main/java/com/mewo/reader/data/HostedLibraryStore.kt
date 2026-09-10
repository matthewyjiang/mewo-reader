package com.mewo.reader.data

import android.net.Uri
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.File

/**
 * Second shelf for a future HTTP backend. Empty until a server exists.
 * Does not read the on-device library.
 */
class HostedLibraryStore : LibraryStore {
    private val _library = MutableStateFlow(LibrarySnapshot())
    override val library: StateFlow<LibrarySnapshot> = _library.asStateFlow()

    override suspend fun load() = Unit

    override suspend fun importFromUri(uri: Uri): BookRecord = notConnected()

    override suspend fun importSample(): BookRecord = notConnected()

    override suspend fun delete(id: String) = Unit

    override suspend fun book(id: String): BookRecord? = null

    override suspend fun feed(id: String): List<FeedPost> = notConnected()

    override suspend fun saveProgress(id: String, index: Int) = Unit

    override suspend fun likes(id: String): Set<String> = emptySet()

    override suspend fun toggleLike(id: String, postId: String): Set<String> = emptySet()

    override suspend fun search(query: String): SearchResult = SearchResult()

    override suspend fun likedPosts(): List<PostHit> = emptyList()

    override fun coverFile(id: String): File? = null

    private fun notConnected(): Nothing = error(HOSTED_NOT_CONNECTED)
}

const val HOSTED_NOT_CONNECTED = "Hosted library has no server yet."
