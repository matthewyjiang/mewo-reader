package com.mewo.reader.ui.search

import android.net.Uri
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.LibrarySnapshot
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.PostHit
import com.mewo.reader.data.PostKind
import com.mewo.reader.data.ProfileReply
import com.mewo.reader.data.SearchResult
import java.io.File
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.withContext
import org.junit.After
import org.junit.Assert.assertEquals
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

        vm.setQuery("a")
        runCurrent()
        vm.setQuery("ab")
        runCurrent()
        assertTrue("first search should be cancelled", store.searches.getValue("a").cancelled)

        store.finishSearch("ab", resultFor("ab"))
        advanceUntilIdle()
        assertEquals("ab", vm.state.value.query)
        assertEquals(resultFor("ab"), vm.state.value.result)
    }

    @Test
    fun staleSearchCannotPublishAfterTheTextReturnsToIt() = runTest(main) {
        // The store ignores cancellation, like a scan with no check in its loop.
        val store = FakeStore(ignoreCancel = true)
        val vm = SearchViewModel(store)

        vm.setQuery("a")
        runCurrent()
        val first = store.searches.getValue("a")
        vm.setQuery("ab")
        vm.setQuery("a")
        runCurrent()

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
    fun hitsShowBeforeLikesAndCommentsLoad() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        vm.setQuery("a")
        runCurrent()
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
    fun socialFromAnOldQueryIsDropped() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        vm.setQuery("a")
        runCurrent()
        store.finishSearch("a", resultFor("a"))
        runCurrent()
        vm.setQuery("ab")
        runCurrent()

        store.likes.getValue(BOOK).complete(setOf("p1"))
        advanceUntilIdle()
        assertEquals(emptyMap<String, Set<String>>(), vm.state.value.likes)
    }

    @Test
    fun aLikeToggleBeatsTheOlderLikesLoad() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)

        vm.setQuery("a")
        runCurrent()
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

        vm.setQuery("a")
        runCurrent()
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
        val two = SearchResult(
            posts = listOf("b1", "b2").map { PostHit(book(it), FeedPost("p1", PostKind.Body, "a", "One")) },
        )

        vm.setQuery("a")
        runCurrent()
        store.finishSearch("a", two)
        runCurrent()
        assertEquals(listOf("likes b1"), store.requests)

        store.likes.getValue("b1").complete(emptySet())
        runCurrent()
        assertEquals(listOf("likes b1", "comments b1"), store.requests)
    }

    private class PendingSearch {
        val result = CompletableDeferred<SearchResult>()
        var cancelled = false
    }

    /** Search, likes, and comments wait until the test completes them. */
    private class FakeStore(private val ignoreCancel: Boolean = false) : LibraryStore {
        val searches = mutableMapOf<String, PendingSearch>()
        val likes = mutableMapOf<String, CompletableDeferred<Set<String>>>()
        val comments = mutableMapOf<String, CompletableDeferred<CommentIndex>>()
        val toggles = mutableMapOf<String, CompletableDeferred<Set<String>>>()
        val requests = mutableListOf<String>()

        fun finishSearch(query: String, result: SearchResult) {
            searches.getValue(query).result.complete(result)
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
            return likes.getOrPut(id) { CompletableDeferred() }.await()
        }

        override suspend fun commentIndex(id: String): CommentIndex {
            requests += "comments $id"
            return comments.getOrPut(id) { CompletableDeferred() }.await()
        }

        override suspend fun toggleLike(id: String, postId: String): Set<String> =
            toggles.getOrPut(id) { CompletableDeferred() }.await()

        override val library: StateFlow<LibrarySnapshot> = MutableStateFlow(LibrarySnapshot())
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

    private companion object {
        const val BOOK = "b1"

        fun book(id: String) = BookRecord(id = id, title = id, author = "Austen", handle = "austen", importedAt = 1)

        fun resultFor(query: String) = SearchResult(
            posts = listOf(PostHit(book(BOOK), FeedPost("p1", PostKind.Body, "text $query", "One"))),
        )
    }
}
