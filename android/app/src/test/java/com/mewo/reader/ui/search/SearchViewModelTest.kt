package com.mewo.reader.ui.search

import android.net.Uri
import com.mewo.reader.data.AccountHit
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.LibraryLoad
import com.mewo.reader.data.LibraryLoading
import com.mewo.reader.data.LibrarySnapshot
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.PostHit
import com.mewo.reader.data.PostKind
import com.mewo.reader.data.ProfileReply
import com.mewo.reader.data.RepostState
import com.mewo.reader.data.SearchKind
import com.mewo.reader.data.SearchPage
import com.mewo.reader.data.SearchResult
import com.mewo.reader.data.hasReposted
import java.io.File
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.withContext
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class SearchViewModelTest {
    private lateinit var main: TestDispatcher

    @Before
    fun setUp() {
        main = StandardTestDispatcher()
        Dispatchers.setMain(main)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun newKeystrokeCancelsTheOlderSearch() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        arm(vm, "a")
        vm.setQuery("ab")
        runCurrent()
        assertTrue("first search should be cancelled", store.searches.getValue("a").cancelled)

        advanceTimeBy(SEARCH_DEBOUNCE_MS)
        runCurrent()
        store.finishSearch("ab", resultFor("ab"))
        advanceUntilIdle()
        assertEquals("ab", vm.state.value.query)
        assertEquals(resultFor("ab"), vm.state.value.result)
    }

    @Test
    fun keystrokesInsideTheDebounceShareOneSearch() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        vm.setQuery("a")
        advanceTimeBy(SEARCH_DEBOUNCE_MS - 1)
        runCurrent()
        vm.setQuery("ab")
        advanceTimeBy(SEARCH_DEBOUNCE_MS - 1)
        runCurrent()
        assertTrue(store.searches.isEmpty())

        advanceTimeBy(1)
        runCurrent()
        assertEquals(setOf("ab"), store.searches.keys)
    }

    @Test
    fun staleSearchCannotPublishAfterTheTextReturnsToIt() = runTest(main) {
        // The store ignores cancellation, like a scan with no check in its loop.
        val store = FakeStore(ignoreCancel = true)
        val vm = SearchViewModel(store)

        arm(vm, "a")
        val first = store.searches.getValue("a")
        arm(vm, "ab")
        arm(vm, "a")

        val stale = SearchResult(books = listOf(book("stale")))
        first.result.complete(stale)
        advanceUntilIdle()
        assertEquals(SearchResult(), vm.state.value.result)

        store.finishSearch("a", resultFor("a"))
        advanceUntilIdle()
        assertEquals("a", vm.state.value.query)
        assertEquals(resultFor("a"), vm.state.value.result)
    }

    @Test
    fun aPendingSocialLoadIsReusedByTheNextQuery() = runTest(main) {
        // Like the hosted client: cancel cannot stop a request already sent.
        val store = FakeStore(ignoreCancel = true)
        val vm = SearchViewModel(store)

        arm(vm, "a")
        store.finishSearch("a", resultFor("a"))
        runCurrent()
        arm(vm, "ab")
        store.finishSearch("ab", resultFor("ab"))
        runCurrent()
        assertEquals(listOf("likes b1"), store.requests)

        store.likes.getValue(BOOK).complete(setOf("p1"))
        runCurrent()
        store.comments.getValue(BOOK).complete(CommentIndex(counts = mapOf("p1" to 2)))
        advanceUntilIdle()
        assertEquals(listOf("likes b1", "comments b1"), store.requests)
        assertEquals(setOf("p1"), vm.state.value.likes[BOOK])
        assertEquals(2, vm.state.value.comments.getValue(BOOK).count("p1"))
    }

    @Test
    fun hitsShowBeforeLikesAndCommentsLoad() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        arm(vm, "a")
        store.finishSearch("a", resultFor("a"))
        runCurrent()
        assertEquals(resultFor("a"), vm.state.value.result)
        assertEquals(emptyMap<String, Set<String>>(), vm.state.value.likes)

        store.likes.getValue(BOOK).complete(setOf("p1"))
        store.comments.getOrPut(BOOK) { CompletableDeferred() }.complete(CommentIndex(counts = mapOf("p1" to 2)))
        advanceUntilIdle()
        assertEquals(setOf("p1"), vm.state.value.likes[BOOK])
        assertEquals(2, vm.state.value.comments.getValue(BOOK).count("p1"))
    }

    @Test
    fun loadedSocialIsReusedAndFailedFieldsRetry() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        arm(vm, "a")
        store.finishSearch("a", resultFor("a"))
        runCurrent()
        store.likes.getValue(BOOK).complete(setOf("p1"))
        runCurrent()
        store.comments.getValue(BOOK).completeExceptionally(RuntimeException("offline"))
        advanceUntilIdle()

        // Likes stay cached; only the failed comments load again.
        arm(vm, "ab")
        store.finishSearch("ab", resultFor("ab"))
        runCurrent()
        assertEquals(listOf("likes b1", "comments b1", "comments b1"), store.requests)
        store.comments.getValue(BOOK).complete(CommentIndex(counts = mapOf("p1" to 2)))
        advanceUntilIdle()
        assertEquals(2, vm.state.value.comments.getValue(BOOK).count("p1"))

        // Coming back reloads both, and a failure keeps what was shown.
        vm.refreshSocial()
        runCurrent()
        store.likes.getValue(BOOK).completeExceptionally(RuntimeException("offline"))
        runCurrent()
        store.comments.getValue(BOOK).completeExceptionally(RuntimeException("offline"))
        advanceUntilIdle()
        assertEquals(setOf("p1"), vm.state.value.likes[BOOK])
        assertEquals(2, vm.state.value.comments.getValue(BOOK).count("p1"))
    }

    @Test
    fun aQueuedLoadIsDroppedWhenItsBookLeavesTheResults() = runTest(main) {
        val store = FakeStore(ignoreCancel = true)
        val vm = SearchViewModel(store)

        arm(vm, "a")
        store.finishSearch("a", resultForBooks("b1", "b2"))
        runCurrent()
        arm(vm, "x")
        store.finishSearch("x", resultForBooks("b1", "b3"))
        runCurrent()

        // b1 is already running, so it finishes; b2 never asks the store.
        store.likes.getValue("b1").complete(emptySet())
        runCurrent()
        store.comments.getValue("b1").complete(CommentIndex())
        runCurrent()
        assertEquals(listOf("likes b1", "comments b1", "likes b3"), store.requests)
    }

    @Test
    fun anOlderToggleAnswerCannotReplaceANewerOne() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        vm.toggleLike(BOOK, "p1")
        runCurrent()
        val first = store.toggles.getValue(BOOK)
        store.toggles.remove(BOOK)
        vm.toggleLike(BOOK, "p1")
        runCurrent()

        store.toggles.getValue(BOOK).complete(emptySet())
        runCurrent()
        first.complete(setOf("p1"))
        advanceUntilIdle()
        assertEquals(emptySet<String>(), vm.state.value.likes[BOOK])
    }

    @Test
    fun aCommentEditSurvivesALoadThatSpansAQueryChange() = runTest(main) {
        val store = FakeStore(ignoreCancel = true)
        val vm = SearchViewModel(store)

        arm(vm, "a")
        store.finishSearch("a", resultFor("a"))
        runCurrent()
        store.likes.getValue(BOOK).complete(emptySet())
        runCurrent()

        val mine = PostComment("c1", "me", "hi", createdAt = 1, mine = true)
        vm.onCommentsChanged(BOOK, "p1", listOf(mine))
        arm(vm, "ab")
        store.finishSearch("ab", resultFor("ab"))
        runCurrent()
        store.comments.getValue(BOOK).complete(CommentIndex(counts = mapOf("p2" to 3)))
        advanceUntilIdle()

        val index = vm.state.value.comments.getValue(BOOK)
        assertEquals(1, index.count("p1"))
        assertTrue(index.commented("p1"))
        assertEquals(3, index.count("p2"))
        assertEquals(listOf("likes b1", "comments b1"), store.requests)
    }

    @Test
    fun aLikeToggleBeatsTheOlderLikesLoad() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        arm(vm, "a")
        store.finishSearch("a", resultFor("a"))
        runCurrent()

        vm.toggleLike(BOOK, "p1")
        runCurrent()
        store.toggles.getValue(BOOK).complete(setOf("p1"))
        runCurrent()
        assertEquals(setOf("p1"), vm.state.value.likes[BOOK])

        store.likes.getValue(BOOK).complete(emptySet())
        advanceUntilIdle()
        assertEquals(setOf("p1"), vm.state.value.likes[BOOK])
    }

    @Test
    fun aCommentEditSurvivesTheOlderCommentLoad() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        arm(vm, "a")
        store.finishSearch("a", resultFor("a"))
        store.likes.getOrPut(BOOK) { CompletableDeferred() }.complete(emptySet())
        runCurrent()

        val mine = PostComment("c1", "me", "hi", createdAt = 1, mine = true)
        vm.onCommentsChanged(BOOK, "p1", listOf(mine))
        store.comments.getValue(BOOK).complete(CommentIndex(counts = mapOf("p2" to 3)))
        advanceUntilIdle()

        val index = vm.state.value.comments.getValue(BOOK)
        assertEquals(1, index.count("p1"))
        assertTrue(index.commented("p1"))
        assertEquals(3, index.count("p2"))
    }

    @Test
    fun socialLoadsOneRequestAtATime() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)
        val two = resultForBooks("b1", "b2")

        arm(vm, "a")
        store.finishSearch("a", two)
        runCurrent()
        assertEquals(listOf("likes b1"), store.requests)

        store.likes.getValue("b1").complete(emptySet())
        runCurrent()
        assertEquals(listOf("likes b1", "comments b1"), store.requests)
    }

    @Test
    fun aBlankOrLoadingQueryIsNotNoMatches() {
        assertFalse(showNoMatches(SearchState()))
        assertFalse(showNoMatches(SearchState(query = "ada", phase = SearchPhase.Loading())))
        val empty = SearchState(
            query = "ada",
            kind = SearchKind.Accounts,
            phase = SearchPhase.Loaded(SearchPage.Accounts(), more = false),
        )
        assertTrue(showNoMatches(empty))
    }

    @Test
    fun signOutDropsAResponseForTheOldSession() = runTest(main) {
        val store = HostedFake(keepGoing = true)
        val vm = SearchViewModel(store)
        vm.bindSession("one")
        arm(vm, "ada")
        vm.bindSession(null)
        store.finish("ada", SearchPage.Accounts(listOf(AccountHit("ada"))))
        advanceUntilIdle()
        assertTrue(vm.state.value.search.phase is SearchPhase.Idle)
    }

    @Test
    fun aNewerQueryDropsTheOlderPage() = runTest(main) {
        val store = HostedFake(keepGoing = true)
        val vm = SearchViewModel(store)
        vm.bindSession("one")
        arm(vm, "a")
        val first = store.pages.getValue("a")
        arm(vm, "ab")
        first.complete(SearchPage.Accounts(listOf(AccountHit("stale"))))
        advanceUntilIdle()
        assertTrue(vm.state.value.search.phase is SearchPhase.Loading)
        store.finish("ab", SearchPage.Accounts(listOf(AccountHit("ab"))))
        advanceUntilIdle()
        val page = (vm.state.value.search.phase as SearchPhase.Loaded).page as SearchPage.Accounts
        assertEquals(listOf("ab"), page.items.map { it.username })
    }

    @Test
    fun aRepostSurvivesTheCommentLoadThatStartedBeforeIt() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)
        arm(vm, "line")
        store.finishSearch("line", resultFor("line"))
        runCurrent()
        store.likes.getValue(BOOK).complete(emptySet())
        runCurrent()
        vm.toggleRepost(BOOK, "p1")
        runCurrent()
        assertTrue(vm.state.value.comments.getValue(BOOK).hasReposted("p1"))
        store.comments.getValue(BOOK).complete(CommentIndex())
        advanceUntilIdle()
        assertTrue(vm.state.value.comments.getValue(BOOK).hasReposted("p1"))
        store.reposts.getValue(BOOK).complete(RepostState(1, true))
        advanceUntilIdle()
        assertTrue(vm.state.value.comments.getValue(BOOK).hasReposted("p1"))
    }

    @Test
    fun repostRollsBackWhenTheServerRejects() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)
        vm.toggleRepost(BOOK, "p1")
        runCurrent()
        assertTrue(vm.state.value.comments.getValue(BOOK).hasReposted("p1"))
        store.reposts.getValue(BOOK).completeExceptionally(RuntimeException("offline"))
        advanceUntilIdle()
        assertFalse(vm.state.value.comments.getValue(BOOK).hasReposted("p1"))
    }

    private fun TestScope.arm(vm: SearchViewModel, query: String) {
        vm.setQuery(query)
        advanceTimeBy(SEARCH_DEBOUNCE_MS)
        runCurrent()
    }

    private class PendingSearch {
        val result = CompletableDeferred<SearchResult>()
        var cancelled = false
    }

    /** Search, likes, and comments wait until the test completes them. */
    private open class FakeStore(private val ignoreCancel: Boolean = false) : LibraryStore {
        val searches = mutableMapOf<String, PendingSearch>()
        val reposts = mutableMapOf<String, CompletableDeferred<RepostState>>()
        val likes = mutableMapOf<String, CompletableDeferred<Set<String>>>()
        val comments = mutableMapOf<String, CompletableDeferred<CommentIndex>>()
        val toggles = mutableMapOf<String, CompletableDeferred<Set<String>>>()
        val requests = mutableListOf<String>()

        fun finishSearch(query: String, result: SearchResult) {
            searches.getValue(query).result.complete(result)
        }

        override suspend fun setRepost(id: String, postId: String, reposted: Boolean): RepostState {
            return answer(reposts, id)
        }

        override suspend fun search(query: String): SearchResult {
            val pending = PendingSearch()
            searches[query] = pending
            if (ignoreCancel) return withContext(NonCancellable) { pending.result.await() }
            return try {
                pending.result.await()
            } catch (e: kotlinx.coroutines.CancellationException) {
                pending.cancelled = true
                throw e
            }
        }

        override suspend fun likes(id: String): Set<String> {
            requests += "likes $id"
            return answer(likes, id)
        }

        override suspend fun commentIndex(id: String): CommentIndex {
            requests += "comments $id"
            return answer(comments, id)
        }

        override suspend fun toggleLike(id: String, postId: String): Set<String> {
            requests += "toggle $id $postId"
            return answer(toggles, id)
        }

        /** Waits on the deferred for [id], then clears it so the next call gets a fresh one. */
        private suspend fun <T> answer(pending: MutableMap<String, CompletableDeferred<T>>, id: String): T {
            val deferred = pending.getOrPut(id) { CompletableDeferred() }
            try {
                return if (ignoreCancel) withContext(NonCancellable) { deferred.await() } else deferred.await()
            } finally {
                pending.remove(id, deferred)
            }
        }

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
        override suspend fun comments(id: String, postId: String): List<PostComment> = emptyList()
        override suspend fun addComment(id: String, postId: String, text: String): List<PostComment> =
            emptyList()
        override suspend fun deleteComment(id: String, postId: String, commentId: String): List<PostComment> =
            emptyList()
        override suspend fun likedPosts(): List<PostHit> = emptyList()
        override suspend fun profileReplies(handle: String): List<ProfileReply> = emptyList()
        override fun coverFile(id: String): File? = null
    }

    private class HostedFake(private val keepGoing: Boolean = false) : FakeStore(keepGoing) {
        override val hostedSocial: Boolean = true
        val pages = mutableMapOf<String, CompletableDeferred<SearchPage>>()

        fun finish(query: String, page: SearchPage) {
            pages.getValue(query).complete(page)
        }

        override suspend fun searchPage(query: String, kind: SearchKind, cursor: String?): SearchPage {
            val pending = CompletableDeferred<SearchPage>()
            pages[query] = pending
            return if (keepGoing) withContext(NonCancellable) { pending.await() } else pending.await()
        }
    }

    private companion object {
        const val BOOK = "b1"

        fun book(id: String) = BookRecord(id = id, title = id, author = "Austen", handle = "austen", importedAt = 1)

        fun resultFor(query: String) = SearchResult(
            posts = listOf(PostHit(book(BOOK), FeedPost("p1", PostKind.Body, "text $query", "One"))),
        )

        fun resultForBooks(vararg ids: String) = SearchResult(
            posts = ids.map { PostHit(book(it), FeedPost("p1", PostKind.Body, "a", "One")) },
        )
    }
}
