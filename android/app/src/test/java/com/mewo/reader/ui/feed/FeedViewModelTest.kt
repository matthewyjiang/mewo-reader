package com.mewo.reader.ui.feed

import android.net.Uri
import com.mewo.reader.data.ActorRecord
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPhase
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.HomeFeedItem
import com.mewo.reader.data.HomeFeedPage
import com.mewo.reader.data.LibraryLoad
import com.mewo.reader.data.LibraryLoading
import com.mewo.reader.data.LibrarySnapshot
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.MemoryFeedCache
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.PostHit
import com.mewo.reader.data.PostKind
import com.mewo.reader.data.ProfileReply
import com.mewo.reader.data.RepostState
import com.mewo.reader.data.SearchResult
import com.mewo.reader.data.hasReposted
import com.mewo.reader.data.showFeedEmpty
import java.io.File
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class FeedViewModelTest {
    private val main = StandardTestDispatcher()

    @Before
    fun setUp() {
        Dispatchers.setMain(main)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun cachedPageShowsBeforeTheRefreshFinishes() = runTest(main) {
        val store = FakeStore()
        val cache = MemoryFeedCache()
        val cached = HomeFeedPage(listOf(repost("byron")))
        cache.write("ada", cached)
        val vm = FeedViewModel(store, cache)

        vm.bindSession("ada")
        runCurrent()
        val shown = vm.state.value.phase as FeedPhase.Loaded
        assertEquals(1, shown.page.items.size)
        assertTrue(shown.refreshing)
        assertFalse(showFeedEmpty(shown))

        store.finish(HomeFeedPage())
        advanceUntilIdle()
        assertTrue(showFeedEmpty(vm.state.value.phase))
    }

    @Test
    fun emptyCopyWaitsForASuccessfulAnswer() = runTest(main) {
        val store = FakeStore()
        val vm = FeedViewModel(store, MemoryFeedCache())
        vm.bindSession("ada")
        runCurrent()
        assertFalse(showFeedEmpty(vm.state.value.phase))
        store.fail(RuntimeException("offline"))
        advanceUntilIdle()
        assertFalse(showFeedEmpty(vm.state.value.phase))
        assertTrue(vm.state.value.phase is FeedPhase.Failed)
    }

    @Test
    fun aFailedRefreshKeepsTheCachedRows() = runTest(main) {
        val store = FakeStore()
        val cache = MemoryFeedCache()
        cache.write("ada", HomeFeedPage(listOf(repost("byron"))))
        val vm = FeedViewModel(store, cache)
        vm.bindSession("ada")
        runCurrent()
        store.fail(RuntimeException("offline"))
        advanceUntilIdle()
        val failed = vm.state.value.phase as FeedPhase.Failed
        assertEquals(1, failed.page?.items?.size)
    }

    @Test
    fun signOutDropsTheVisiblePage() = runTest(main) {
        val store = FakeStore()
        val cache = MemoryFeedCache()
        cache.write("ada", HomeFeedPage(listOf(repost("byron"))))
        val vm = FeedViewModel(store, cache)
        vm.bindSession("ada")
        runCurrent()
        vm.bindSession(null)
        store.finish(HomeFeedPage(listOf(repost("late"))))
        advanceUntilIdle()
        assertTrue(vm.state.value.phase is FeedPhase.Loading)
    }

    @Test
    fun localModeIsAnEmptySuccess() = runTest(main) {
        val store = FakeStore(hosted = false)
        val vm = FeedViewModel(store, MemoryFeedCache())
        vm.bindSession("local")
        runCurrent()
        assertTrue(showFeedEmpty(vm.state.value.phase))
    }

    @Test
    fun loadMoreDuringRefreshDoesNotAppendTheCachedCursor() = runTest(main) {
        val store = FakeStore()
        val cache = MemoryFeedCache()
        cache.write("ada", HomeFeedPage(listOf(repost("byron")), nextCursor = "c2"))
        val vm = FeedViewModel(store, cache)
        vm.bindSession("ada")
        runCurrent()
        vm.loadMore()
        runCurrent()
        val during = vm.state.value.phase as FeedPhase.Loaded
        assertTrue(during.refreshing)
        assertFalse(during.more)
        assertEquals(1, during.page.items.size)
        store.finish(HomeFeedPage(listOf(repost("ada"))))
        advanceUntilIdle()
        val done = vm.state.value.phase as FeedPhase.Loaded
        assertEquals(1, done.page.items.size)
        assertFalse(done.refreshing)
    }

    @Test
    fun aCommentLoadDoesNotDropARepostThatStartedAfterIt() = runTest(main) {
        val store = FakeStore()
        val vm = FeedViewModel(store, MemoryFeedCache())
        vm.bindSession("ada")
        runCurrent()
        store.finish(HomeFeedPage(listOf(repost("byron"))))
        runCurrent()
        store.likes.complete(emptySet())
        runCurrent()
        vm.toggleRepost("b1", "0")
        runCurrent()
        assertTrue(vm.state.value.comments.getValue("b1").hasReposted("0"))
        store.comments.complete(CommentIndex())
        advanceUntilIdle()
        assertTrue(vm.state.value.comments.getValue("b1").hasReposted("0"))
        store.reposts.complete(RepostState(1, true))
        advanceUntilIdle()
        assertTrue(vm.state.value.comments.getValue("b1").hasReposted("0"))
    }

    @Test
    fun repostRollsBackWhenTheServerRejects() = runTest(main) {
        val store = FakeStore()
        val cache = MemoryFeedCache()
        cache.write("ada", HomeFeedPage(listOf(repost("byron"))))
        val vm = FeedViewModel(store, cache)
        vm.bindSession("ada")
        advanceUntilIdle()
        store.comments.complete(CommentIndex())
        store.likes.complete(emptySet())
        advanceUntilIdle()

        vm.toggleRepost("b1", "0")
        runCurrent()
        assertTrue(vm.state.value.comments.getValue("b1").hasReposted("0"))
        store.reposts.completeExceptionally(RuntimeException("offline"))
        advanceUntilIdle()
        assertFalse(vm.state.value.comments.getValue("b1").hasReposted("0"))
    }

    private fun repost(username: String) = HomeFeedItem.Repost(
        actor = ActorRecord(username),
        book = BookRecord(id = "b1", title = "Emma", author = "Austen", handle = "austen", importedAt = 1),
        post = FeedPost("0", PostKind.Body, "A line.", "One"),
        at = 10,
    )

    private class FakeStore(private val hosted: Boolean = true) : LibraryStore {
        override val hostedSocial: Boolean = hosted
        val feed = CompletableDeferred<HomeFeedPage>()
        val likes = CompletableDeferred<Set<String>>()
        val comments = CompletableDeferred<CommentIndex>()
        val reposts = CompletableDeferred<RepostState>()

        fun finish(page: HomeFeedPage) { feed.complete(page) }
        fun fail(error: Exception) { feed.completeExceptionally(error) }

        override suspend fun homeFeed(cursor: String?): HomeFeedPage = feed.await()
        override suspend fun likes(id: String): Set<String> = likes.await()
        override suspend fun commentIndex(id: String): CommentIndex = comments.await()
        override suspend fun setRepost(id: String, postId: String, reposted: Boolean): RepostState =
            reposts.await()

        override val library: StateFlow<LibrarySnapshot> = MutableStateFlow(LibrarySnapshot())
        override val loadState: StateFlow<LibraryLoad> = MutableStateFlow(LibraryLoading)
        override suspend fun load() = Unit
        override suspend fun importFromUri(uri: Uri): BookRecord = error("unused")
        override suspend fun importSample(): BookRecord = error("unused")
        override suspend fun delete(id: String) = Unit
        override suspend fun setShared(id: String, shared: Boolean) = Unit
        override suspend fun book(id: String): BookRecord? = null
        override suspend fun feed(id: String): List<FeedPost> = emptyList()
        override suspend fun saveProgress(id: String, index: Int) = Unit
        override suspend fun search(query: String): SearchResult = SearchResult()
        override suspend fun comments(id: String, postId: String): List<PostComment> = emptyList()
        override suspend fun addComment(id: String, postId: String, text: String): List<PostComment> =
            emptyList()
        override suspend fun deleteComment(id: String, postId: String, commentId: String): List<PostComment> =
            emptyList()
        override suspend fun toggleLike(id: String, postId: String): Set<String> = emptySet()
        override suspend fun likedPosts(): List<PostHit> = emptyList()
        override suspend fun profileReplies(handle: String): List<ProfileReply> = emptyList()
        override fun coverFile(id: String): File? = null
    }
}
