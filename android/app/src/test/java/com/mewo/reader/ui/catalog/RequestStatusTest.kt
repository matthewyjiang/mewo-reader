package com.mewo.reader.ui.catalog

import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogPage
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.data.Rating
import com.mewo.reader.data.RequestDetail
import com.mewo.reader.data.RequestPhase
import com.mewo.reader.data.ServerCopy
import com.mewo.reader.data.WorkDetail
import com.mewo.reader.data.WorkInfo
import com.mewo.reader.data.WorkStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RequestStatusTest {
    @Test
    fun phaseMatchesTheServerTable() {
        val cases = listOf(
            Triple("requested", null as String?, 0) to RequestPhase.Searching,
            Triple("requested", "No release yet.", 0) to RequestPhase.Waiting,
            Triple(
                "requested",
                "No matching release (closest: Dune Messiah, a different book)",
                0,
            ) to RequestPhase.NoMatch,
            Triple("requested", "SABnzbd: down", 0) to RequestPhase.Held,
            Triple("requested", "Prowlarr: down", 0) to RequestPhase.Held,
            Triple("requested", "No EPUB in the download.", 0) to RequestPhase.Retrying,
            Triple("requested", "Download failed: CRC error", 0) to RequestPhase.Retrying,
            Triple("requested", "", 0) to RequestPhase.Searching,
            Triple("downloading", "Open Library: HTTP 500", 40) to RequestPhase.Held,
            Triple("downloading", null, 40) to RequestPhase.Downloading,
            Triple("downloading", null, 100) to RequestPhase.Importing,
            Triple("downloading", "SABnzbd: paused", 100) to RequestPhase.Held,
            Triple("failed", "No release found.", 0) to RequestPhase.Failed,
            Triple("canceled", null, 0) to RequestPhase.Canceled,
            Triple("imported", null, 100) to RequestPhase.Imported,
        )
        cases.forEach { (input, expected) ->
            val (state, error, progress) = input
            assertEquals(
                "$state/$error/$progress",
                expected,
                requestPhase(state, progress, error, phase = null),
            )
        }
        assertEquals(
            RequestPhase.Imported,
            requestPhase("requested", 0, "No release yet.", phase = "imported"),
        )
        assertTrue(RequestPhase.Downloading.live)
        assertFalse(RequestPhase.Failed.live)
        assertFalse(RequestPhase.Imported.live)
    }

    @Test
    fun aListRowKeepsTheOlderLabelUntilTheServerSendsAPhase() {
        val retrying = book(state = "requested", error = "No EPUB in the download.")
        assertEquals(RequestPhase.Retrying, requestPhase(retrying))
        assertEquals("Searching", stateLabel(retrying))
        val waiting = book(state = "requested", error = "No release yet.")
        assertEquals("Waiting for a release", stateLabel(waiting))
        val detail = request(
            state = "requested",
            error = "No EPUB in the download.",
            phase = "retrying",
        )
        assertEquals("Retrying", stateLabel(detail))
    }

    @Test
    fun aHeldCopyNeverNamesItsOwner() {
        val held = ServerCopy(bookId = "b1", ownerUsername = "secret", held = true, mine = false)
        assertEquals("On your shelf", copyLabel(held, CopyStyle.Badge))
        assertEquals("On your shelf", copyLabel(held, CopyStyle.Sheet))
        assertFalse(copyLabel(held, CopyStyle.Sheet).contains("secret"))

        val work = work(onServer = held)
        assertEquals("On your shelf", badge(work, null))
        assertEquals(RowAction.Open, rowAction(null, held))
        assertEquals(RowAction.Pending, rowAction(book(state = "requested"), null))
        assertEquals(RowAction.Pending, rowAction(book(state = "downloading"), null))
        assertEquals(RowAction.Retry, rowAction(book(state = "failed"), null))
        assertEquals(RowAction.Again, rowAction(book(state = "canceled"), null))
        assertEquals(RowAction.Open, rowAction(book(state = "imported", bookId = "b1"), null))
        assertEquals(RowAction.Again, rowAction(book(state = "imported"), null))
        assertEquals(RowAction.Request, rowAction(null, null))
        val status = sheetStatus(work, null, null) as SheetStatus.Available
        assertEquals("On your shelf", status.line)
        assertFalse(status.line.contains("secret"))
        assertEquals("b1", status.bookId)

        val shared = ServerCopy(bookId = "b2", ownerUsername = "ada", shared = true)
        assertEquals("On server · @ada", copyLabel(shared, CopyStyle.Badge))
        assertEquals("On server, shared by @ada", copyLabel(shared, CopyStyle.Sheet))
    }

    @Test
    fun alsoRequestedUsesOnlyTheNamesTheServerSent() {
        assertEquals("Also requested by @bea", alsoRequested(listOf("bea")))
        assertEquals(
            "Also requested by @bea, @ada, @cam +2",
            alsoRequested(listOf("bea", "ada", "cam", "dee", "eve")),
        )
        val others = listOf(
            book(id = "r2", requester = "bea"),
            book(id = "r3", requester = "ada"),
        )
        val detail = WorkDetail(
            workKey = KEY,
            status = WorkStatus(otherRequests = others, cap = 5),
        )
        val state = CatalogSheetState(work = work(), detail = detail, loading = false)
        assertEquals("Also requested by @bea, @ada", state.alsoRequested)
        val quiet = state.copy(detail = detail.copy(status = detail.status.copy(otherRequests = emptyList())))
        assertNull(quiet.alsoRequested)
    }

    @Test
    fun fewerThanThreeScalarsStayOnTrendingIncludingAnEmoji() {
        assertTrue(catalogQuery("") is CatalogQuery.Browse)
        assertTrue(catalogQuery("  ") is CatalogQuery.Browse)
        assertTrue(catalogQuery("a") is CatalogQuery.Browse)
        assertTrue(catalogQuery("  a ") is CatalogQuery.Browse)
        assertTrue(catalogQuery("ab") is CatalogQuery.Browse)
        assertTrue(catalogQuery("📚") is CatalogQuery.Browse)
        assertTrue(catalogQuery("📚a") is CatalogQuery.Browse)
        assertEquals(CatalogQuery.Search("abc"), catalogQuery("  abc  "))
        assertEquals(CatalogQuery.Search("📚ab"), catalogQuery("📚ab"))
    }

    @Test
    fun trendingHeaderShowsForBrowseWhileLoadingOrFilled() {
        val rows = CatalogPage(items = listOf(CatalogWork(workKey = "/works/OL1W", title = "Dune")))
        val empty = CatalogPage()
        assertTrue(showTrendingHeader("", CatalogPhase.Loading()))
        assertTrue(showTrendingHeader("du", CatalogPhase.Loaded(rows)))
        assertFalse(showTrendingHeader("", CatalogPhase.Loaded(empty)))
        assertFalse(showTrendingHeader("", CatalogPhase.Failed("Couldn't load the catalog.")))
        assertFalse(showTrendingHeader("dune", CatalogPhase.Loaded(rows)))
    }

    @Test
    fun errorTextNeverShowsARawStatus() {
        val rendered = listOf(
            catalogErrorText(500, "Server returned 500", network = false),
            catalogErrorText(502, "HTTP 502", network = false),
            catalogErrorText(500, "HTTP/1.1 500 Internal Server Error", network = false),
            catalogErrorText(0, null, network = true),
            catalogErrorText(401, "Server returned 401", network = false),
            catalogErrorText(403, null, network = false),
            catalogErrorText(404, "HTTP 404", network = false),
            catalogErrorText(403, "Requests are off.", network = false),
        )
        rendered.forEach { text ->
            assertFalse(text, text.contains("HTTP"))
            assertFalse(text, text.contains("Server returned"))
        }
        assertEquals("Requests are off.", catalogErrorText(403, "Requests are off.", false))
        assertEquals("Sign in again.", catalogErrorText(401, "Server returned 401", false))
        assertEquals("That book is no longer in the catalog.", catalogErrorText(404, "HTTP 404", false))
        assertEquals(
            "Couldn't reach the server. Check your connection.",
            catalogErrorText(0, "Server returned 0", network = true),
        )
        assertEquals(
            "Couldn't load the catalog.",
            catalogErrorText(500, "Server returned 500", false),
        )
        assertEquals(
            "Open Library is busy. Try again.",
            catalogErrorText(502, "Open Library is busy. Try again.", false),
        )
        assertFalse(catalogErrorText(0, null, true).endsWith("Try again."))
        assertFalse(catalogErrorText(500, null, false).endsWith("Try again."))
    }

    @Test
    fun anImportedRequestWithNoBookAsksToRequestAgain() {
        val status = sheetStatus(work(), null, book(state = "imported")) as SheetStatus.RequestAgain
        assertNull(status.capNote)
        val open = sheetStatus(work(), null, book(state = "imported", bookId = "b1")) as SheetStatus.Available
        assertEquals("On your shelf", open.line)
        assertEquals("b1", open.bookId)
        val copied = sheetStatus(
            work(onServer = ServerCopy(bookId = "b2", ownerUsername = "ada", shared = true)),
            null,
            book(state = "imported"),
        ) as SheetStatus.Available
        assertEquals("b2", copied.bookId)
        val capped = sheetStatus(
            work(),
            WorkDetail(
                KEY,
                status = WorkStatus(
                    request = request(state = "imported", error = null, phase = "imported"),
                    active = 5,
                    cap = 5,
                ),
            ),
            null,
        ) as SheetStatus.RequestAgain
        assertEquals("You have 5 requests in progress", capped.capNote)
    }

    @Test
    fun statsAndSubjectsStayOnOneLine() {
        val info = WorkInfo(
            canonicalKey = KEY,
            title = "Dune",
            year = 1965,
            pagesMedian = 412,
            editionCount = 99,
            subjects = listOf("Science fiction", "Desert", "Politics", "Ecology", "Arrakis", "Extra"),
            rating = Rating(average = 4.25, count = 1000),
            hasAcceptedEdition = false,
        )
        val stats = workStats(info)
        assertEquals("First published 1965 · ~412 pages · ★ 4.3", stats)
        assertFalse(stats.contains("99"))
        assertFalse(stats.contains("1000"))
        assertEquals(
            "Science fiction · Desert · Politics · Ecology · Arrakis",
            subjectLine(info.subjects),
        )
        assertNull(coverUrl(null))
        assertNull(coverUrl(-1))
        assertEquals(
            "https://covers.openlibrary.org/b/id/9-M.jpg?default=false",
            coverUrl(9),
        )
        val status = sheetStatus(work(), WorkDetail(KEY, info = info, status = WorkStatus(cap = 5)), null)
        val requestable = status as SheetStatus.Requestable
        assertEquals("No English edition listed on Open Library", requestable.editionNote)
        assertNull(requestable.capNote)
        val capped = sheetStatus(
            work(),
            WorkDetail(KEY, info = info, status = WorkStatus(active = 5, cap = 5)),
            null,
        ) as SheetStatus.Requestable
        assertEquals("You have 5 requests in progress", capped.capNote)
    }

    private fun work(onServer: ServerCopy? = null) = CatalogWork(
        workKey = KEY,
        title = "Dune",
        author = "Frank Herbert",
        year = 1965,
        onServer = onServer,
    )

    private fun book(
        id: String = "r1",
        state: String = "requested",
        error: String? = null,
        requester: String = "ada",
        bookId: String? = null,
    ) = BookRequest(
        id = id,
        workKey = KEY,
        title = "Dune",
        state = state,
        error = error,
        requester = requester,
        bookId = bookId,
    )

    private fun request(state: String, error: String?, phase: String) = RequestDetail(
        id = "r1",
        workKey = KEY,
        title = "Dune",
        state = state,
        error = error,
        phase = phase,
    )

    private companion object {
        const val KEY = "/works/OL123W"
    }
}
