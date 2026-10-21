package com.mewo.reader.ui.catalog

import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogException
import com.mewo.reader.data.CatalogPage
import com.mewo.reader.data.CatalogWork
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class CatalogFeedTest {
    @Test
    fun aShortQueryStaysOnTrendingAndSearchWaitsOutTheDebounce() = runTest {
        val store = FakeCatalogStore()
        store.pageHandler = { query, _ -> page(query.ifEmpty { "Trending" }) }
        val feed = feed(store, this)
        feed.bindSession("ada")
        feed.show()
        runCurrent()
        assertEquals(listOf("" to 1), store.pageCalls)

        feed.setQuery("e")
        feed.setQuery("em")
        feed.setQuery("📚")
        feed.setQuery("📚a")
        advanceTimeBy(1_000)
        runCurrent()
        assertEquals(listOf("" to 1), store.pageCalls)

        feed.setQuery("emm")
        advanceTimeBy(CATALOG_DEBOUNCE_MS - 1)
        runCurrent()
        assertTrue(store.pageCalls.none { it.first == "emm" })
        feed.setQuery("emma")
        advanceTimeBy(CATALOG_DEBOUNCE_MS - 1)
        runCurrent()
        assertTrue(store.pageCalls.none { it.first == "emma" })
        advanceTimeBy(1)
        runCurrent()
        assertEquals(1, store.pageCalls.count { it.first == "emma" })
        assertEquals("emma", (feed.state.value.phase as CatalogPhase.Loaded).page.items.single().title)
        feed.hide()
    }

    @Test
    fun aStalePageCannotReplaceTheQueryThatReplacedIt() = runTest {
        val store = FakeCatalogStore()
        val emma = CompletableDeferred<CatalogPage>()
        store.pageHandler = { query, _ ->
            if (query == "emma") {
                withContext(NonCancellable) { emma.await() }
            } else {
                page(query.ifEmpty { "Trending" })
            }
        }
        val feed = feed(store, this)
        feed.bindSession("ada")
        feed.show()
        runCurrent()
        feed.setQuery("emma")
        advanceTimeBy(CATALOG_DEBOUNCE_MS)
        runCurrent()
        feed.setQuery("dune")
        advanceTimeBy(CATALOG_DEBOUNCE_MS)
        runCurrent()
        assertEquals("dune", loaded(feed).items.single().title)

        emma.complete(page("STALE"))
        runCurrent()
        assertEquals("dune", loaded(feed).items.single().title)
        feed.hide()
    }

    @Test
    fun aSinglePageMemoRefreshesAndALongerOneDoesNot() = runTest {
        val store = FakeCatalogStore()
        store.pageHandler = { _, pageNumber ->
            if (pageNumber == 1) page("A", next = 2) else page("B")
        }
        val feed = feed(store, this)
        feed.bindSession("ada")
        feed.show()
        runCurrent()
        assertEquals("A", loaded(feed).items.single().title)

        feed.hide()
        val afterFirst = store.pageCalls.size
        feed.show()
        assertEquals("A", loaded(feed).items.single().title)
        assertEquals(afterFirst, store.pageCalls.size)
        runCurrent()
        assertEquals(afterFirst + 1, store.pageCalls.size)

        feed.loadMore()
        runCurrent()
        assertEquals(listOf("A", "B"), loaded(feed).items.map { it.title })
        val afterMore = store.pageCalls.size
        feed.hide()
        feed.show()
        assertEquals(listOf("A", "B"), loaded(feed).items.map { it.title })
        runCurrent()
        assertEquals(afterMore, store.pageCalls.size)
        feed.hide()
    }

    @Test
    fun aFailureReplacesTheListAndDropsTheMemo() = runTest {
        val store = FakeCatalogStore()
        store.pageHandler = { _, _ -> page("Trending") }
        val feed = feed(store, this)
        feed.bindSession("ada")
        feed.show()
        runCurrent()
        feed.hide()

        store.pageHandler = { _, _ ->
            throw CatalogException(
                CatalogException.Reason.Failed,
                "Couldn't load the catalog.",
            )
        }
        feed.show()
        assertTrue(feed.state.value.phase is CatalogPhase.Loaded)
        runCurrent()
        val failed = feed.state.value.phase as CatalogPhase.Failed
        assertEquals("Couldn't load the catalog.", failed.message)
        assertFalse(failed.message.contains("HTTP"))

        feed.hide()
        feed.show()
        assertTrue(feed.state.value.phase is CatalogPhase.Loading)
        runCurrent()
        feed.hide()
    }

    @Test
    fun loadingAnotherPageFailsByReplacingTheWholeList() = runTest {
        val store = FakeCatalogStore()
        store.pageHandler = { _, pageNumber ->
            if (pageNumber == 1) page("A", next = 2)
            else {
                throw CatalogException(
                    CatalogException.Reason.Network,
                    "Couldn't reach the server. Check your connection.",
                )
            }
        }
        val feed = feed(store, this)
        feed.bindSession("ada")
        feed.show()
        runCurrent()
        feed.loadMore()
        runCurrent()
        assertTrue(feed.state.value.phase is CatalogPhase.Failed)
        feed.hide()
    }

    @Test
    fun eachSessionKeepsItsOwnTrendingMemo() = runTest {
        val store = FakeCatalogStore()
        store.pageHandler = { query, _ -> page(if (query.isEmpty()) "Trending" else query) }
        val feed = feed(store, this)
        feed.bindSession("ada")
        feed.show()
        runCurrent()
        val calls = store.pageCalls.size
        feed.bindSession("bea")
        assertTrue(feed.state.value.phase is CatalogPhase.Loading)
        runCurrent()
        assertEquals(calls + 1, store.pageCalls.size)
        feed.bindSession("ada")
        assertEquals("Trending", loaded(feed).items.single().title)
        feed.hide()
    }

    @Test
    fun aCanceledRowRequestsAgainThroughRetry() = runTest {
        val store = FakeCatalogStore()
        val work = CatalogWork(
            workKey = "/works/OL1W",
            title = "Dune",
            author = "Herbert",
            request = BookRequest(id = "r1", workKey = "/works/OL1W", title = "Dune", state = "canceled"),
        )
        store.retryHandler = {
            BookRequest(id = "r1", workKey = "/works/OL1W", title = "Dune", state = "requested")
        }
        val feed = feed(store, this)
        assertEquals(RowAction.Again, rowAction(work.request, work.onServer))
        feed.act(work)
        assertEquals(listOf("r1"), store.retries)
        assertTrue(store.requestCalls.isEmpty())
    }

    @Test
    fun aRequestWithNoMessageUsesTheCatalogNotice() = runTest {
        val store = FakeCatalogStore()
        val work = CatalogWork(workKey = "/works/OL1W", title = "Dune", author = "Herbert")
        store.requestHandler = { throw Exception() }
        val feed = feed(store, this)
        feed.act(work)
        assertEquals("Couldn't request that book.", feed.state.value.notice)
    }

    @Test
    fun anImportedRowWithNoBookRequestsAgain() = runTest {
        val store = FakeCatalogStore()
        val work = CatalogWork(
            workKey = "/works/OL1W",
            title = "Dune",
            author = "Herbert",
            request = BookRequest(id = "r1", workKey = "/works/OL1W", title = "Dune", state = "imported"),
        )
        store.requestHandler = { asked ->
            BookRequest(id = "r2", workKey = asked.workKey, title = asked.title, state = "requested")
        }
        val feed = feed(store, this)
        feed.act(work)
        assertEquals(listOf("/works/OL1W"), store.requestCalls)
        assertTrue(store.retries.isEmpty())
    }

    private fun feed(store: FakeCatalogStore, scope: CoroutineScope) =
        CatalogFeed(store, scope, onImported = {})

    private fun loaded(feed: CatalogFeed) = (feed.state.value.phase as CatalogPhase.Loaded).page

    private fun page(title: String, next: Int? = null) = CatalogPage(
        items = listOf(CatalogWork(workKey = "/works/$title", title = title, author = "Herbert")),
        nextPage = next,
    )
}
