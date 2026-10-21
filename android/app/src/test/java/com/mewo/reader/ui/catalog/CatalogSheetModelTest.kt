package com.mewo.reader.ui.catalog

import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogException
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.data.RequestDetail
import com.mewo.reader.data.RequestPhase
import com.mewo.reader.data.WorkDetail
import com.mewo.reader.data.WorkStatus
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
class CatalogSheetModelTest {
    @Test
    fun aLivePhasePollsUntilItStops() = runTest {
        val store = FakeCatalogStore()
        var pulls = 0
        store.workHandler = {
            pulls += 1
            if (pulls < 3) detail(request("requested", "searching"))
            else detail(request("imported", "imported", bookId = "b1"))
        }
        val model = model(store, this)
        model.start()
        runCurrent()
        assertEquals(1, pulls)
        advanceTimeBy(CATALOG_POLL_MS)
        runCurrent()
        assertEquals(2, pulls)
        advanceTimeBy(CATALOG_POLL_MS)
        runCurrent()
        assertEquals(3, pulls)
        assertTrue(model.state.value.status is SheetStatus.Available)
        advanceTimeBy(CATALOG_POLL_MS * 3)
        runCurrent()
        assertEquals(3, pulls)
        model.stop()
    }

    @Test
    fun unauthorizedOffAndMissingStopThePoll() = runTest {
        val reasons = listOf(
            CatalogException.Reason.Unauthorized to "Sign in again.",
            CatalogException.Reason.Off to "Requests are off.",
            CatalogException.Reason.NotFound to "That book is no longer in the catalog.",
        )
        reasons.forEach { (reason, message) ->
            val store = FakeCatalogStore()
            store.workHandler = { throw CatalogException(reason, message) }
            val model = model(store, this)
            model.start()
            runCurrent()
            assertEquals(message, model.state.value.loadError)
            val calls = store.workCalls.size
            advanceTimeBy(CATALOG_POLL_MS * 3)
            runCurrent()
            assertEquals(calls, store.workCalls.size)
            model.stop()
        }
    }

    @Test
    fun cancelFlipsToCanceledBeforeTheServerAnswers() = runTest {
        val store = FakeCatalogStore()
        val gate = CompletableDeferred<Unit>()
        val seen = mutableListOf<BookRequest?>()
        store.workHandler = { detail(request("downloading", "downloading", progress = 40)) }
        val model = model(store, this, onRequest = { seen += it })
        model.start()
        runCurrent()
        store.cancelHandler = { gate.await() }
        model.cancel()
        assertEquals("canceled", model.state.value.detail?.status?.request?.state)
        assertEquals(RequestPhase.Canceled.wire, model.state.value.detail?.status?.request?.phase)
        assertEquals("canceled", seen.last()?.state)
        assertTrue(model.state.value.acting)
        assertTrue(store.cancels.isEmpty())

        runCurrent()
        assertEquals(listOf("r1"), store.cancels)
        store.workHandler = { detail(request("canceled", "canceled")) }
        gate.complete(Unit)
        runCurrent()
        assertFalse(model.state.value.acting)
        assertTrue(model.state.value.status is SheetStatus.Canceled)
        model.stop()
    }

    @Test
    fun retryRefreshesFromTheServer() = runTest {
        val store = FakeCatalogStore()
        store.workHandler = {
            detail(request("failed", "failed", error = "The download failed."))
        }
        val model = model(store, this)
        model.start()
        runCurrent()
        assertTrue(model.state.value.status is SheetStatus.Failed)
        store.retryHandler = { book("requested") }
        store.workHandler = { detail(request("requested", "searching")) }
        model.retry()
        runCurrent()
        assertEquals(listOf("r1"), store.retries)
        val status = model.state.value.status as SheetStatus.Live
        assertEquals(RequestPhase.Searching, status.phase)
        assertEquals("Looking for a release…", status.line)
        model.stop()
    }

    @Test
    fun anAlreadyImportedBookDoesNotReloadTheShelf() = runTest {
        val store = FakeCatalogStore()
        var imports = 0
        store.workHandler = { detail(request("imported", "imported", bookId = "b1")) }
        val model = model(store, this, onImported = { imports += 1 })
        model.start()
        runCurrent()
        assertEquals(0, imports)
        advanceTimeBy(CATALOG_POLL_MS * 2)
        runCurrent()
        assertEquals(0, imports)
        assertEquals(1, store.workCalls.size)
        model.stop()
    }

    @Test
    fun importReloadsTheShelfOnceOnTheTransition() = runTest {
        val store = FakeCatalogStore()
        var pulls = 0
        var imports = 0
        store.workHandler = {
            pulls += 1
            if (pulls == 1) detail(request("downloading", "downloading", progress = 10))
            else detail(request("imported", "imported", bookId = "b9"))
        }
        val model = model(store, this, onImported = { imports += 1 })
        model.start()
        runCurrent()
        assertEquals(0, imports)
        advanceTimeBy(CATALOG_POLL_MS)
        runCurrent()
        assertEquals(1, imports)
        assertEquals(2, pulls)
        advanceTimeBy(CATALOG_POLL_MS * 2)
        runCurrent()
        assertEquals(1, imports)
        assertEquals(2, pulls)
        model.stop()
    }

    @Test
    fun requestingAnImportedBookReloadsOnce() = runTest {
        val store = FakeCatalogStore()
        var imports = 0
        store.workHandler = { detail(null) }
        val model = model(store, this, onImported = { imports += 1 })
        model.start()
        runCurrent()
        store.requestHandler = { book("imported", bookId = "b1") }
        store.workHandler = { detail(request("imported", "imported", bookId = "b1")) }
        model.request()
        runCurrent()
        assertEquals(1, imports)
        model.stop()
    }

    @Test
    fun aPullOlderThanTheActionIsDropped() = runTest {
        val store = FakeCatalogStore()
        val first = CompletableDeferred<WorkDetail>()
        var calls = 0
        store.workHandler = {
            calls += 1
            if (calls == 1) {
                withContext(NonCancellable) { first.await() }
            } else {
                detail(request("failed", "failed", error = "The download failed."))
            }
        }
        val model = model(store, this)
        model.start()
        runCurrent()
        store.requestHandler = { book("failed", error = "The download failed.") }
        model.request()
        runCurrent()
        assertTrue(model.state.value.status is SheetStatus.Failed)
        first.complete(detail(request("downloading", "downloading", progress = 99)))
        runCurrent()
        assertEquals("failed", model.state.value.detail?.status?.request?.state)
        model.stop()
    }

    private fun model(
        store: FakeCatalogStore,
        scope: CoroutineScope,
        onRequest: (BookRequest?) -> Unit = {},
        onImported: suspend () -> Unit = {},
    ) = CatalogSheetModel(
        work = CatalogWork(workKey = KEY, title = "Dune", author = "Herbert"),
        request = null,
        store = store,
        scope = scope,
        onRequest = onRequest,
        onImported = onImported,
    )

    private fun detail(request: RequestDetail?) = WorkDetail(
        workKey = KEY,
        status = WorkStatus(request = request, active = 0, cap = 5),
    )

    private fun request(
        state: String,
        phase: String,
        progress: Int = 0,
        error: String? = null,
        bookId: String? = null,
    ) = RequestDetail(
        id = "r1",
        workKey = KEY,
        title = "Dune",
        author = "Herbert",
        state = state,
        progress = progress,
        error = error,
        bookId = bookId,
        phase = phase,
    )

    private fun book(state: String, error: String? = null, bookId: String? = null) = BookRequest(
        id = "r1",
        workKey = KEY,
        title = "Dune",
        author = "Herbert",
        state = state,
        error = error,
        bookId = bookId,
    )

    private companion object {
        const val KEY = "/works/OL123W"
    }
}
