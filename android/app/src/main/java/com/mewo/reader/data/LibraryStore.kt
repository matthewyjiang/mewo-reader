package com.mewo.reader.data

import android.net.Uri
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import java.io.File

/**
 * Library verbs the UI uses. Local keeps books on the device. Hosted
 * talks to a server. Same calls either way. Local books are parsed on
 * the phone; hosted books are parsed by the server. A hosted book stays
 * private until the owner shares it.
 */
interface LibraryStore {
    val library: StateFlow<LibrarySnapshot>

    /** Loading, loaded (optionally refreshing), or failed with an optional saved shelf. */
    val loadState: StateFlow<LibraryLoad>

    suspend fun load()

    /** Drop a failed-with-cache banner once the reader has dismissed it. */
    fun dismissLoadError() {}

    suspend fun importFromUri(uri: Uri): BookRecord

    suspend fun importSample(): BookRecord

    suspend fun delete(id: String)

    /** Owner-only on the hosted library. Local has no shared shelf. */
    suspend fun setShared(id: String, shared: Boolean)

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

    /** Hosted paged search. Local keeps [search] and has no accounts tab. */
    val hostedSocial: Boolean get() = false

    suspend fun searchPage(query: String, kind: SearchKind, cursor: String?): SearchPage =
        error("Search pages are hosted only.")

    suspend fun homeFeed(cursor: String?): HomeFeedPage = error("The home feed is hosted only.")

    suspend fun profile(username: String): ProfileRecord = error("Profiles are hosted only.")

    suspend fun profilePosts(username: String, cursor: String?): ProfilePostsPage =
        error("Profiles are hosted only.")

    suspend fun profileReposts(username: String, cursor: String?): ProfileRepostsPage =
        error("Profiles are hosted only.")

    suspend fun profileBooks(username: String): ProfileBooks = error("Profiles are hosted only.")

    suspend fun setRepost(id: String, postId: String, reposted: Boolean): RepostState =
        error("Reposts are hosted only.")

    suspend fun likedPosts(): List<PostHit>

    /** Replies by this handle, newest first, each with the line they answered. */
    suspend fun profileReplies(handle: String): List<ProfileReply>

    /** Local path the avatar can decode. Hosted writes a cache file first. */
    fun coverFile(id: String): File?

    /** Hosted only. Empty map on the local backend (local books are already on the phone). */
    val offline: StateFlow<Map<String, OfflineState>>
        get() = NO_OFFLINE

    val offlineSummary: StateFlow<OfflineSummary>
        get() = NO_SUMMARY

    /** on = fetch posts, cover and social now; off = delete posts.json and social.json. Idempotent. */
    suspend fun setDownloaded(id: String, on: Boolean) {}

    /**
     * Downloads every shelf book that is not downloaded yet, two at a time, in shelf order.
     * Tap again while that run is going and it cancels. One shot, not a saved setting.
     */
    suspend fun downloadAll() {}
}

private val NO_OFFLINE: StateFlow<Map<String, OfflineState>> = MutableStateFlow(emptyMap())
private val NO_SUMMARY: StateFlow<OfflineSummary> =
    MutableStateFlow(OfflineSummary(downloaded = 0, total = 0, running = false))

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

    override val loadState: StateFlow<LibraryLoad> = kind
        .flatMapLatest { which -> active(which).loadState }
        .stateIn(scope, SharingStarted.Eagerly, LibraryLoading)

    override val offline: StateFlow<Map<String, OfflineState>> = kind
        .flatMapLatest { which -> active(which).offline }
        .stateIn(scope, SharingStarted.Eagerly, emptyMap())

    override val offlineSummary: StateFlow<OfflineSummary> = kind
        .flatMapLatest { which -> active(which).offlineSummary }
        .stateIn(scope, SharingStarted.Eagerly, OfflineSummary(0, 0, false))

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

    override suspend fun setShared(id: String, shared: Boolean) = active().setShared(id, shared)

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

    override val hostedSocial: Boolean get() = active().hostedSocial

    override suspend fun searchPage(query: String, kind: SearchKind, cursor: String?) =
        active().searchPage(query, kind, cursor)

    override suspend fun homeFeed(cursor: String?) = active().homeFeed(cursor)

    override suspend fun profile(username: String) = active().profile(username)

    override suspend fun profilePosts(username: String, cursor: String?) =
        active().profilePosts(username, cursor)

    override suspend fun profileReposts(username: String, cursor: String?) =
        active().profileReposts(username, cursor)

    override suspend fun profileBooks(username: String) = active().profileBooks(username)

    override suspend fun setRepost(id: String, postId: String, reposted: Boolean) =
        active().setRepost(id, postId, reposted)

    override suspend fun likedPosts() = active().likedPosts()

    override suspend fun profileReplies(handle: String) = active().profileReplies(handle)

    override fun coverFile(id: String) = active().coverFile(id)

    override suspend fun setDownloaded(id: String, on: Boolean) = active().setDownloaded(id, on)

    override suspend fun downloadAll() = active().downloadAll()
}
