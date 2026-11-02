package com.mewo.reader.ui.search

import android.net.Uri
import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogAccess
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.data.AccountHit
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.HostedException
import com.mewo.reader.data.LibraryLoad
import com.mewo.reader.data.LibraryLoading
import com.mewo.reader.data.LibrarySnapshot
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.PostHit
import com.mewo.reader.data.PostHitRecord
import com.mewo.reader.data.PostKind
import com.mewo.reader.data.ProfileReply
import com.mewo.reader.data.RepostState
import com.mewo.reader.data.SearchKind
import com.mewo.reader.data.SearchPage
import com.mewo.reader.data.SearchResult
import com.mewo.reader.data.hasReposted
import com.mewo.reader.ui.catalog.CATALOG_DEBOUNCE_MS
import com.mewo.reader.ui.catalog.FakeCatalogStore
import java.io.File
import java.io.IOException
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
    fun aBlankHostedQueryLoadsTheBrowsePage() = runTest(main) {
        val store = HostedFake()
        val vm = SearchViewModel(store)
        vm.bindSession("t")
        advanceTimeBy(0)
        runCurrent()
        assertEquals(listOf(SearchCall("", SearchKind.Accounts, null)), store.calls)
        assertTrue(vm.state.value.search.phase is SearchPhase.Loading)
        store.finish("", SearchPage.Accounts(listOf(AccountHit("ada"))))
        advanceUntilIdle()
        assertEquals(listOf("ada"), accountsOf(vm))
        assertTrue(vm.state.value.search.phase is SearchPhase.Loaded)
    }

    @Test
    fun aBlankLocalQueryStaysIdle() = runTest(main) {
        val store = FakeStore()
        val vm = SearchViewModel(store)
        vm.bindSession("local")
        vm.refreshBrowse()
        vm.setKind(SearchKind.Posts)
        advanceTimeBy(0)
        runCurrent()
        assertTrue(vm.state.value.search.phase is SearchPhase.Idle)
        assertTrue(store.searches.isEmpty())

        vm.setQuery("a")
        advanceTimeBy(SEARCH_DEBOUNCE_MS)
        runCurrent()
        store.finishSearch("a", resultFor("a"))
        advanceUntilIdle()
        assertEquals(resultFor("a"), vm.state.value.result)

        vm.setQuery("")
        assertTrue(vm.state.value.search.phase is SearchPhase.Idle)
        assertEquals(SearchResult(), vm.state.value.result)
        assertEquals(setOf("a"), store.searches.keys)
    }

    @Test
    fun switchingTabsUsesTheBrowseMemoUntilItIsStale() = runTest(main) {
        var clock = 1_000L
        val store = HostedFake()
        val vm = SearchViewModel(store, now = { clock })
        vm.bindSession("t")
        advanceTimeBy(0)
        runCurrent()
        store.finish("", SearchPage.Accounts(listOf(AccountHit("ada"))))
        advanceUntilIdle()
        vm.setKind(SearchKind.Books)
        advanceTimeBy(0)
        runCurrent()
        store.finish("", SearchPage.Books(listOf(book("b1"))))
        advanceUntilIdle()
        assertEquals(2, store.calls.size)

        vm.setKind(SearchKind.Accounts)
        assertEquals(listOf("ada"), accountsOf(vm))
        advanceTimeBy(0)
        runCurrent()
        assertEquals(2, store.calls.size)

        clock = 1_000L + BROWSE_REVALIDATE_MS
        vm.setKind(SearchKind.Books)
        advanceTimeBy(0)
        runCurrent()
        vm.setKind(SearchKind.Accounts)
        advanceTimeBy(0)
        runCurrent()
        assertEquals(2, store.calls.size)

        clock = 1_000L + BROWSE_REVALIDATE_MS + 1
        vm.setKind(SearchKind.Books)
        assertEquals(listOf("b1"), booksOf(vm))
        advanceTimeBy(0)
        runCurrent()
        assertEquals(3, store.calls.size)
        assertEquals(SearchKind.Books, store.calls.last().kind)
        store.finish("", SearchPage.Books(listOf(book("b2"))))
        advanceUntilIdle()

        vm.setKind(SearchKind.Accounts)
        assertEquals(listOf("ada"), accountsOf(vm))
        advanceTimeBy(0)
        runCurrent()
        assertEquals(4, store.calls.size)
        assertEquals(SearchKind.Accounts, store.calls.last().kind)
        store.finish("", SearchPage.Accounts(listOf(AccountHit("bea"))))
        advanceUntilIdle()
        assertEquals(listOf("bea"), accountsOf(vm))
    }

    @Test
    fun aFailedQuietRefreshKeepsThePage() = runTest(main) {
        var clock = 3_000L
        val store = HostedFake()
        val vm = SearchViewModel(store, now = { clock })
        vm.bindSession("t")
        advanceTimeBy(0)
        runCurrent()
        store.finish("", SearchPage.Accounts(listOf(AccountHit("ada"))))
        advanceUntilIdle()
        clock += BROWSE_REVALIDATE_MS + 1
        vm.refreshBrowse()
        advanceTimeBy(0)
        runCurrent()
        store.fail("", IOException("offline"))
        advanceUntilIdle()
        val phase = vm.state.value.search.phase as SearchPhase.Loaded
        assertEquals(listOf("ada"), (phase.page as SearchPage.Accounts).items.map { it.username })
    }

    @Test
    fun clearingTheFieldShowsTheBrowseMemo() = runTest(main) {
        val store = HostedFake()
        val vm = SearchViewModel(store, now = { 5_000L })
        vm.bindSession("t")
        advanceTimeBy(0)
        runCurrent()
        store.finish("", SearchPage.Accounts(listOf(AccountHit("ada"))))
        advanceUntilIdle()

        vm.setQuery("bea")
        advanceTimeBy(SEARCH_DEBOUNCE_MS)
        runCurrent()
        store.finish("bea", SearchPage.Accounts(listOf(AccountHit("bea"))))
        advanceUntilIdle()
        assertEquals(listOf("bea"), accountsOf(vm))

        vm.setQuery("")
        assertEquals(listOf("ada"), accountsOf(vm))
        assertEquals(2, store.calls.size)

        vm.setQuery("cy")
        val loading = vm.state.value.search.phase as SearchPhase.Loading
        assertEquals(listOf("ada"), (loading.previous as SearchPage.Accounts).items.map { it.username })
        advanceTimeBy(SEARCH_DEBOUNCE_MS)
        runCurrent()
        assertTrue(vm.state.value.search.phase is SearchPhase.Loading)
        store.finish("cy", SearchPage.Accounts(listOf(AccountHit("cy"))))
        advanceUntilIdle()
        assertEquals(listOf("cy"), accountsOf(vm))
    }

    @Test
    fun aNewSessionDropsTheBrowseMemo() = runTest(main) {
        val store = HostedFake(keepGoing = true)
        val vm = SearchViewModel(store)
        vm.bindSession("one")
        advanceTimeBy(0)
        runCurrent()
        val first = store.pages.getValue("")
        vm.bindSession("two")
        first.complete(SearchPage.Accounts(listOf(AccountHit("stale"))))
        runCurrent()
        assertTrue(vm.state.value.search.phase is SearchPhase.Loading)
        val second = store.pages.getValue("")
        second.complete(SearchPage.Accounts(listOf(AccountHit("fresh"))))
        advanceUntilIdle()
        assertEquals(listOf("fresh"), accountsOf(vm))

        vm.bindSession("one")
        assertTrue(vm.state.value.search.phase is SearchPhase.Loading)
        advanceTimeBy(0)
        runCurrent()
        store.finish("", SearchPage.Accounts(listOf(AccountHit("one"))))
        advanceUntilIdle()
        assertEquals(listOf("one"), accountsOf(vm))

        vm.bindSession(null)
        assertTrue(vm.state.value.search.phase is SearchPhase.Idle)
    }

    @Test
    fun aColdBrowseFailureUsesTheCatalogNetworkSentence() = runTest(main) {
        val store = HostedFake()
        val vm = SearchViewModel(store)
        vm.bindSession("t")
        advanceTimeBy(0)
        runCurrent()
        store.fail("", IOException("failed to connect"))
        advanceUntilIdle()
        val failed = vm.state.value.search.phase as SearchPhase.Failed
        assertEquals("Couldn't reach the server. Check your connection.", failed.error)
        assertEquals(null, failed.previous)

        vm.retry()
        advanceTimeBy(0)
        runCurrent()
        assertEquals(2, store.calls.size)
        assertEquals("", store.calls.last().query)
        assertEquals(SearchKind.Accounts, store.calls.last().kind)
        store.finish("", SearchPage.Accounts(listOf(AccountHit("ada"))))
        advanceUntilIdle()
        assertEquals(listOf("ada"), accountsOf(vm))
    }

    @Test
    fun otherSearchFailuresKeepTheirMessage() = runTest(main) {
        val store = HostedFake()
        val vm = SearchViewModel(store)
        vm.bindSession("t")
        advanceTimeBy(0)
        runCurrent()
        store.fail("", HostedException("No such book.", 404))
        advanceUntilIdle()
        val failed = vm.state.value.search.phase as SearchPhase.Failed
        assertEquals("No such book.", failed.error)
        assertEquals(null, failed.previous)
    }

    @Test
    fun refreshBrowseRetriesAFailureAndAStalePage() = runTest(main) {
        var clock = 2_000L
        val store = HostedFake()
        val vm = SearchViewModel(store, now = { clock })
        vm.bindSession("t")
        advanceTimeBy(0)
        runCurrent()
        assertTrue(vm.state.value.search.phase is SearchPhase.Loading)
        assertEquals(1, store.calls.size)
        vm.refreshBrowse()
        advanceTimeBy(0)
        runCurrent()
        assertEquals(1, store.calls.size)

        store.fail("", IOException("timeout"))
        advanceUntilIdle()
        assertTrue(vm.state.value.search.phase is SearchPhase.Failed)

        vm.refreshBrowse()
        advanceTimeBy(0)
        runCurrent()
        assertEquals(2, store.calls.size)
        assertTrue(vm.state.value.search.phase is SearchPhase.Loading)
        store.finish("", SearchPage.Accounts(listOf(AccountHit("ada"))))
        advanceUntilIdle()

        vm.refreshBrowse()
        advanceTimeBy(0)
        runCurrent()
        assertEquals(2, store.calls.size)

        clock += BROWSE_REVALIDATE_MS + 1
        vm.refreshBrowse()
        assertEquals(listOf("ada"), accountsOf(vm))
        advanceTimeBy(0)
        runCurrent()
        assertEquals(3, store.calls.size)
        store.finish("", SearchPage.Accounts(listOf(AccountHit("bea"))))
        advanceUntilIdle()
        assertEquals(listOf("bea"), accountsOf(vm))
    }

    @Test
    fun aPostsBrowseMemoLoadsLikes() = runTest(main) {
        val store = HostedFake()
        val vm = SearchViewModel(store, now = { 0L })
        vm.bindSession("t")
        advanceTimeBy(0)
        runCurrent()
        store.finish("", SearchPage.Accounts(listOf(AccountHit("ada"))))
        advanceUntilIdle()
        vm.setKind(SearchKind.Posts)
        advanceTimeBy(0)
        runCurrent()
        val post = SearchPage.Posts(
            listOf(PostHitRecord(book(BOOK), FeedPost("p1", PostKind.Body, "line", "One"))),
        )
        store.finish("", post)
        advanceUntilIdle()
        runCurrent()
        assertEquals(listOf("likes b1"), store.requests)
        store.likes.getValue(BOOK).completeExceptionally(RuntimeException("offline"))
        runCurrent()
        store.comments.getValue(BOOK).complete(CommentIndex())
        advanceUntilIdle()
        assertEquals(listOf("likes b1", "comments b1"), store.requests)

        vm.setKind(SearchKind.Accounts)
        advanceTimeBy(0)
        runCurrent()
        assertEquals(2, store.calls.size)
        vm.setKind(SearchKind.Posts)
        advanceTimeBy(0)
        runCurrent()
        assertEquals(2, store.calls.size)
        assertEquals(listOf("likes b1", "comments b1", "likes b1"), store.requests)
    }

    @Test
    fun aBlankQueryLoadsAnotherPage() = runTest(main) {
        val store = HostedFake()
        val vm = SearchViewModel(store)
        vm.bindSession("t")
        advanceTimeBy(0)
        runCurrent()
        store.finish("", SearchPage.Accounts(listOf(AccountHit("ada")), nextCursor = "c2"))
        advanceUntilIdle()
        vm.loadMore()
        runCurrent()
        assertEquals(SearchCall("", SearchKind.Accounts, "c2"), store.calls.last())
        store.finish("", SearchPage.Accounts(listOf(AccountHit("bea"))))
        advanceUntilIdle()
        val phase = vm.state.value.search.phase as SearchPhase.Loaded
        val page = phase.page as SearchPage.Accounts
        assertEquals(listOf("ada", "bea"), page.items.map { it.username })
        assertFalse(phase.more)
    }

    @Test
    fun emptyKindMatchesBrowseAndTypedMisses() {
        assertEquals(EmptyKind.None, emptyKind(SearchState()))
        assertEquals(
            EmptyKind.None,
            emptyKind(SearchState(query = "ada", phase = SearchPhase.Loading())),
        )
        assertEquals(
            EmptyKind.None,
            emptyKind(
                SearchState(
                    query = "ada",
                    phase = SearchPhase.Loaded(SearchPage.Accounts(), more = true),
                ),
            ),
        )
        assertEquals(
            EmptyKind.None,
            emptyKind(
                SearchState(
                    query = "ada",
                    kind = SearchKind.Books,
                    phase = SearchPhase.Loaded(SearchPage.Accounts(), more = false),
                ),
            ),
        )
        assertEquals(EmptyKind.None, emptyKind(SearchState(phase = SearchPhase.Idle)))
        val browseEmpty = SearchState(
            query = "  ",
            kind = SearchKind.Accounts,
            phase = SearchPhase.Loaded(SearchPage.Accounts(), more = false),
        )
        assertEquals(EmptyKind.NothingYet, emptyKind(browseEmpty))
        val typedEmpty = SearchState(
            query = "ada",
            kind = SearchKind.Accounts,
            phase = SearchPhase.Loaded(SearchPage.Accounts(), more = false),
        )
        assertEquals(EmptyKind.NoMatches, emptyKind(typedEmpty))
        val typedHits = typedEmpty.copy(
            phase = SearchPhase.Loaded(SearchPage.Accounts(listOf(AccountHit("ada"))), more = false),
        )
        assertEquals(EmptyKind.None, emptyKind(typedHits))
    }

    @Test
    fun sectionTitlesAndEmptyCopyMatchTheWeb() {
        assertEquals("Suggested accounts", sectionTitle(SearchKind.Accounts))
        assertEquals("Popular on this server", sectionTitle(SearchKind.Books))
        assertEquals("Top posts", sectionTitle(SearchKind.Posts))
        assertEquals("Trending on Open Library", sectionTitle(SearchKind.Catalog))
        assertEquals(
            "It's just you here so far.",
            emptyCopy(SearchKind.Accounts, EmptyKind.NothingYet, ""),
        )
        assertEquals("No books yet.", emptyCopy(SearchKind.Books, EmptyKind.NothingYet, ""))
        assertEquals(
            "No posts yet. Replies and reposts on books show up here.",
            emptyCopy(SearchKind.Posts, EmptyKind.NothingYet, ""),
        )
        assertEquals(
            "No accounts match “ada”",
            emptyCopy(SearchKind.Accounts, EmptyKind.NoMatches, "ada"),
        )
        assertEquals(
            "No books match “dune”",
            emptyCopy(SearchKind.Books, EmptyKind.NoMatches, " dune "),
        )
        assertEquals(
            "No posts match “hi”",
            emptyCopy(SearchKind.Posts, EmptyKind.NoMatches, "hi"),
        )
        assertEquals(
            "Couldn't reach the server. Check your connection.",
            searchErrorText(IOException("timeout")),
        )
        assertEquals("Server returned 500", searchErrorText(IllegalStateException("Server returned 500")))
        assertEquals("Could not search.", searchErrorText(IllegalStateException(" ")))
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

    @Test
    fun searchTabsAddCatalogOnlyWhenRequestsAreOn() {
        assertEquals(
            listOf("books" to "Books", "posts" to "Posts"),
            searchTabs(hosted = false, canRequest = false),
        )
        assertEquals(
            listOf("books" to "Books", "posts" to "Posts", "catalog" to "Catalog"),
            searchTabs(hosted = false, canRequest = true),
        )
        assertEquals(
            listOf("accounts" to "Accounts", "books" to "Books", "posts" to "Posts"),
            searchTabs(hosted = true, canRequest = false),
        )
        assertEquals(
            listOf(
                "accounts" to "Accounts",
                "books" to "Books",
                "posts" to "Posts",
                "catalog" to "Catalog",
            ),
            searchTabs(hosted = true, canRequest = true),
        )
    }

    @Test
    fun catalogNeverAsksTheLibrarySearch() = runTest(main) {
        val store = HostedFake()
        val catalog = FakeCatalogStore()
        val vm = SearchViewModel(store, catalogStore = catalog)
        runCurrent()
        vm.bindSession("token")
        vm.setKind(SearchKind.Catalog)
        runCurrent()
        assertEquals(SearchKind.Catalog, vm.state.value.search.kind)
        assertTrue(vm.state.value.search.phase is SearchPhase.Idle)
        assertEquals(listOf("" to 1), catalog.pageCalls)

        vm.setQuery("e")
        vm.setQuery("em")
        advanceTimeBy(1_000)
        runCurrent()
        assertEquals(listOf("" to 1), catalog.pageCalls)
        assertTrue(store.pages.isEmpty())

        vm.setQuery("emma")
        advanceTimeBy(CATALOG_DEBOUNCE_MS)
        runCurrent()
        assertEquals("emma", catalog.pageCalls.last().first)
        assertTrue(store.pages.isEmpty())

        vm.setQuery("")
        advanceTimeBy(0)
        runCurrent()
        assertTrue(store.pages.isEmpty())
        assertTrue(store.calls.isEmpty())
    }

    @Test
    fun requestsTurningOffLeavesTheCatalogTab() = runTest(main) {
        val store = HostedFake()
        val catalog = FakeCatalogStore()
        val memory = MemorySearchKind()
        memory.write(SearchKind.Catalog)
        val vm = SearchViewModel(store, memory, catalog)
        runCurrent()
        assertEquals(SearchKind.Catalog, vm.state.value.search.kind)
        catalog.setAccess(CatalogAccess.Off)
        runCurrent()
        assertEquals(SearchKind.Accounts, vm.state.value.search.kind)
        assertEquals(listOf(SearchCall("", SearchKind.Accounts, null)), store.calls)
        store.finish("", SearchPage.Accounts())
        advanceUntilIdle()
    }

    @Test
    fun anOfflineStartDoesNotOpenAMissingCatalogTab() = runTest(main) {
        val store = HostedFake()
        val catalog = FakeCatalogStore()
        catalog.setAccess(CatalogAccess.Unknown)
        val memory = MemorySearchKind()
        memory.write(SearchKind.Catalog)
        val vm = SearchViewModel(store, memory, catalog)
        runCurrent()
        assertEquals(SearchKind.Accounts, vm.state.value.search.kind)
        catalog.setAccess(CatalogAccess.On)
        runCurrent()
        assertEquals(SearchKind.Catalog, vm.state.value.search.kind)
    }

    @Test
    fun aCatalogRowRequestRunsWithTheViewModel() = runTest(main) {
        val catalog = FakeCatalogStore()
        val vm = SearchViewModel(HostedFake(), catalogStore = catalog)
        runCurrent()
        vm.bindSession("token")
        vm.setKind(SearchKind.Catalog)
        runCurrent()
        val work = CatalogWork(workKey = "/works/OL1W", title = "Dune", author = "Herbert")
        catalog.requestHandler = { asked ->
            BookRequest(id = "r1", workKey = asked.workKey, title = asked.title, state = "requested")
        }
        vm.actOnCatalog(work) {}
        runCurrent()
        assertEquals(listOf("/works/OL1W"), catalog.requestCalls)
    }

    @Test
    fun reloadLibraryRethrowsCancellation() = runTest {
        try {
            reloadLibrary { throw kotlinx.coroutines.CancellationException("stop") }
            error("cancellation was swallowed")
        } catch (error: kotlinx.coroutines.CancellationException) {
            assertEquals("stop", error.message)
        }
        reloadLibrary { throw java.io.IOException("disk") }
    }

    @Test
    fun aLocalReaderDoesNotRestoreTheCatalogTab() = runTest(main) {
        val memory = MemorySearchKind()
        memory.write(SearchKind.Catalog)
        val vm = SearchViewModel(FakeStore(), memory)
        runCurrent()
        assertEquals(SearchKind.Books, vm.state.value.search.kind)
    }

    private fun accountsOf(vm: SearchViewModel): List<String> {
        val page = (vm.state.value.search.phase as SearchPhase.Loaded).page as SearchPage.Accounts
        return page.items.map { it.username }
    }

    private fun booksOf(vm: SearchViewModel): List<String> {
        val page = (vm.state.value.search.phase as SearchPhase.Loaded).page as SearchPage.Books
        return page.items.map { it.id }
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
        val calls = mutableListOf<SearchCall>()

        fun finish(query: String, page: SearchPage) {
            pages.getValue(query).complete(page)
        }

        fun fail(query: String, error: Exception) {
            pages.getValue(query).completeExceptionally(error)
        }

        override suspend fun searchPage(query: String, kind: SearchKind, cursor: String?): SearchPage {
            calls += SearchCall(query, kind, cursor)
            val pending = CompletableDeferred<SearchPage>()
            pages[query] = pending
            return if (keepGoing) withContext(NonCancellable) { pending.await() } else pending.await()
        }
    }

    private data class SearchCall(val query: String, val kind: SearchKind, val cursor: String?)

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
