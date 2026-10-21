import XCTest
@testable import Mewo

final class CatalogCopyTests: XCTestCase {
    func testPhaseMatchesTheServerRule() {
        let rows: [(BookRequest, String)] = [
            (bookRequest(), "searching"),
            (bookRequest(error: "No release yet."), "waiting"),
            (bookRequest(error: "No matching release (closest: Dune Messiah, a different book)"), "no_match"),
            (bookRequest(error: "SABnzbd: down"), "held"),
            (bookRequest(error: "Prowlarr: down"), "held"),
            (bookRequest(error: "No EPUB in the download."), "retrying"),
            (bookRequest(error: "Download failed: CRC error"), "retrying"),
            (bookRequest(error: ""), "searching"),
            (bookRequest(state: "downloading", progress: 40, error: "Open Library: HTTP 500"), "held"),
            (bookRequest(state: "downloading", progress: 40), "downloading"),
            (bookRequest(state: "downloading", progress: 100), "importing"),
            (bookRequest(state: "downloading", progress: 100, error: "SABnzbd: paused"), "held"),
            (bookRequest(state: "failed", error: "No release found."), "failed"),
            (bookRequest(state: "canceled"), "canceled"),
            (bookRequest(state: "imported", bookId: "req-1"), "imported"),
        ]
        for (request, raw) in rows {
            XCTAssertEqual(requestPhase(request).rawValue, raw, request.error ?? request.state)
        }
        // A detail phase wins, including held over a finished download.
        XCTAssertEqual(
            requestPhase(bookRequest(state: "downloading", progress: 40), phase: "importing"),
            .importing
        )
        XCTAssertEqual(RequestPhase.noMatch.rawValue, "no_match")
        XCTAssertTrue(RequestPhase.searching.isLive)
        XCTAssertTrue(RequestPhase.waiting.isLive)
        XCTAssertTrue(RequestPhase.noMatch.isLive)
        XCTAssertTrue(RequestPhase.held.isLive)
        XCTAssertTrue(RequestPhase.retrying.isLive)
        XCTAssertTrue(RequestPhase.downloading.isLive)
        XCTAssertTrue(RequestPhase.importing.isLive)
        XCTAssertFalse(RequestPhase.failed.isLive)
        XCTAssertFalse(RequestPhase.canceled.isLive)
        XCTAssertFalse(RequestPhase.imported.isLive)
    }

    func testStateLabelBadgeAndRowAction() {
        XCTAssertEqual(stateLabel(bookRequest()), "Searching")
        XCTAssertEqual(stateLabel(bookRequest(error: "No release yet.")), "Waiting for a release")
        XCTAssertEqual(stateLabel(bookRequest(state: "downloading", progress: 42)), "Downloading 42%")
        XCTAssertEqual(stateLabel(bookRequest(state: "imported", bookId: "req-1")), "On your shelf")
        XCTAssertEqual(
            stateLabel(bookRequest(state: "failed", error: "No EPUB in the download.")),
            "Failed: No EPUB in the download."
        )
        XCTAssertEqual(stateLabel(bookRequest(state: "canceled")), "Canceled")
        XCTAssertEqual(
            stateLabel(bookRequest(state: "downloading", progress: 100, error: "SABnzbd: paused"), phase: "held"),
            "Waiting on SABnzbd"
        )

        XCTAssertEqual(rowAction(nil), .request)
        XCTAssertEqual(rowAction(bookRequest()), .pending)
        XCTAssertEqual(rowAction(bookRequest(state: "downloading", progress: 42)), .pending)
        XCTAssertEqual(rowAction(bookRequest(state: "imported", bookId: "req-1")), .open)
        XCTAssertEqual(rowAction(bookRequest(state: "imported")), .retry)
        XCTAssertEqual(rowAction(bookRequest(state: "failed", error: "x")), .retry)
        XCTAssertEqual(rowAction(bookRequest(state: "canceled")), .retry)
        XCTAssertEqual(rowAction(nil, sharedCopy()), .open)
        XCTAssertEqual(rowAction(bookRequest(), sharedCopy()), .pending)
        XCTAssertEqual(rowAction(bookRequest(state: "failed", error: "x"), sharedCopy()), .retry)
        XCTAssertEqual(rowAction(bookRequest(state: "canceled"), sharedCopy()), .open)

        XCTAssertEqual(openLabel(work(onServer: sharedCopy()), nil), "On server")
        XCTAssertEqual(openLabel(work(onServer: heldCopy()), nil), "On your shelf")
        XCTAssertEqual(openLabel(work(), bookRequest(state: "imported", bookId: "req-1")), "On your shelf")
    }

    func testCopyHidesAHeldOwner() {
        XCTAssertEqual(copyLabel(heldCopy(), .badge), "On your shelf")
        XCTAssertEqual(copyLabel(heldCopy(), .sheet), "On your shelf")
        XCTAssertFalse(copyLabel(heldCopy(), .badge).contains("secret"))
        XCTAssertFalse(copyLabel(heldCopy(), .sheet).contains("secret"))
        var sharedHeld = heldCopy()
        sharedHeld.shared = true
        XCTAssertEqual(copyLabel(sharedHeld, .sheet), "On your shelf")
        XCTAssertFalse(copyLabel(sharedHeld, .sheet).contains("secret"))
        XCTAssertEqual(copyLabel(sharedCopy(), .badge), "On server · @alice")
        XCTAssertEqual(copyLabel(sharedCopy(), .sheet), "On server, shared by @alice")
        var unnamed = sharedCopy()
        unnamed.ownerUsername = ""
        XCTAssertEqual(copyLabel(unnamed, .badge), "On server")

        XCTAssertEqual(badge(work(onServer: sharedCopy()), bookRequest()), "Searching")
        XCTAssertEqual(badge(work(onServer: sharedCopy()), bookRequest(state: "canceled")), "On server · @alice")
        XCTAssertEqual(badge(work(onServer: heldCopy()), nil), "On your shelf")
        XCTAssertNil(badge(work(), nil))
        let row = catalogRowLabel(work(onServer: heldCopy()), nil)
        XCTAssertEqual(row, "Dune, Frank Herbert, 1965, On your shelf")
        XCTAssertFalse(row.contains("secret"))

        XCTAssertEqual(alsoRequested(["lin"]), "Also requested by @lin")
        XCTAssertEqual(alsoRequested(["lin", "ada", "noor"]), "Also requested by @lin, @ada, @noor")
        XCTAssertEqual(alsoRequested(["lin", "ada", "noor", "grace"]), "Also requested by @lin, @ada, @noor +1")
    }

    func testSheetCopyAndSingleStatusLine() {
        let closest = "No matching release (closest: Dune Messiah, a different book)"
        XCTAssertTrue(noMatchCopy(closest, nil).contains("Dune Messiah"))
        XCTAssertTrue(noMatchCopy(closest, 1_700_000_000_000).contains("Searching again"))
        XCTAssertEqual(waitingCopy(nil), "No release yet.")
        XCTAssertEqual(heldCopy("SABnzbd: paused"), "Waiting on SABnzbd: paused")
        XCTAssertEqual(heldCopy("Prowlarr: down"), "Waiting on Prowlarr: down")

        let cap = "You have 5 requests in progress"
        XCTAssertEqual(
            sheetStatus(
                phase: .searching,
                request: bookRequest(),
                detailRequest: nil,
                copy: nil,
                bookId: nil,
                capNote: cap,
                hasAcceptedEdition: true
            ),
            .live(phase: .searching, line: "Looking for a release…", progress: nil)
        )
        XCTAssertEqual(
            sheetStatus(
                phase: .downloading,
                request: bookRequest(state: "downloading", progress: 40),
                detailRequest: nil,
                copy: nil,
                bookId: nil,
                capNote: cap,
                hasAcceptedEdition: true
            ),
            .live(phase: .downloading, line: "Downloading 40%", progress: 40)
        )
        XCTAssertEqual(
            sheetStatus(
                phase: .importing,
                request: bookRequest(state: "downloading", progress: 100),
                detailRequest: nil,
                copy: nil,
                bookId: nil,
                capNote: nil,
                hasAcceptedEdition: true
            ),
            .live(phase: .importing, line: "Checking and importing…", progress: 100)
        )
        XCTAssertEqual(
            sheetStatus(
                phase: .retrying,
                request: bookRequest(error: ""),
                detailRequest: nil,
                copy: nil,
                bookId: nil,
                capNote: nil,
                hasAcceptedEdition: true
            ),
            .live(phase: .retrying, line: "Trying another release…", progress: nil)
        )
        XCTAssertEqual(
            sheetStatus(
                phase: .failed,
                request: bookRequest(state: "failed"),
                detailRequest: nil,
                copy: nil,
                bookId: nil,
                capNote: cap,
                hasAcceptedEdition: true
            ),
            .failed(line: "The download failed.", capNote: cap)
        )
        XCTAssertEqual(
            sheetStatus(
                phase: .canceled,
                request: bookRequest(state: "canceled"),
                detailRequest: nil,
                copy: nil,
                bookId: nil,
                capNote: cap,
                hasAcceptedEdition: true
            ),
            .canceled(capNote: cap)
        )
        XCTAssertEqual(
            sheetStatus(
                phase: nil,
                request: nil,
                detailRequest: nil,
                copy: nil,
                bookId: nil,
                capNote: cap,
                hasAcceptedEdition: false
            ),
            .requestable(capNote: cap, editionNote: "No English edition listed on Open Library")
        )
        let available = sheetStatus(
            phase: .imported,
            request: bookRequest(state: "imported", bookId: "b"),
            detailRequest: nil,
            copy: heldCopy(),
            bookId: "b",
            capNote: cap,
            hasAcceptedEdition: true
        )
        XCTAssertEqual(available, .available(line: "On your shelf", bookId: "b"))
        if case .available(let line, _) = available {
            XCTAssertFalse(line.contains("secret"))
        }

        XCTAssertEqual(
            workStats(workInfo()),
            "First published 1965 · ~608 pages · ★ 4.3"
        )
        XCTAssertFalse(workStats(workInfo()).contains("400"))
        XCTAssertFalse(workStats(workInfo()).contains("eng"))
        XCTAssertEqual(
            subjectLine(["Science fiction", "Arrakis", "Deserts", "Sand", "Politics", "Ecology"]),
            "Science fiction · Arrakis · Deserts · Sand · Politics"
        )
        XCTAssertNil(coverUrl(nil))
        XCTAssertNil(coverUrl(-1))
        XCTAssertEqual(coverUrl(0), "https://covers.openlibrary.org/b/id/0-M.jpg?default=false")
        XCTAssertEqual(capNote(WorkStatus(request: nil, onServer: nil, active: 5, cap: 5)), cap)
        XCTAssertNil(capNote(WorkStatus(request: nil, onServer: nil, active: 4, cap: 5)))
    }

    func testQueryGateTreatsOneScalarAsTrending() {
        XCTAssertEqual(catalogQuery(""), .browse)
        XCTAssertEqual(catalogQuery("  \n"), .browse)
        XCTAssertEqual(catalogQuery("a"), .browse)
        XCTAssertEqual(catalogQuery("  b "), .browse)
        XCTAssertEqual(catalogQuery("📚"), .browse)
        XCTAssertEqual(catalogQuery("📚📚"), .search("📚📚"))
        XCTAssertEqual(catalogQuery("  dune  "), .search("dune"))
        XCTAssertEqual(catalogHeading(""), "Trending on Open Library")
        XCTAssertEqual(catalogHeading("   "), "Trending on Open Library")
        XCTAssertNil(catalogHeading("a"))
        XCTAssertNil(catalogHeading("📚"))
        XCTAssertNil(catalogHeading("dune"))
        XCTAssertTrue(catalogEmptyCopy("dune").contains("“dune”"))
        XCTAssertEqual(catalogDebounceNanos, 300_000_000)
        XCTAssertEqual(catalogPollNanos, 10_000_000_000)
        XCTAssertEqual(catalogMinScalars, 2)
    }

    func testErrorTextNeverShowsARawStatus() {
        let network = catalogErrorText(code: nil, serverMessage: "Server returned 0", network: true)
        XCTAssertEqual(network, "Could not reach the server. Check your connection and try again.")
        XCTAssertFalse(network.contains("Server returned"))
        XCTAssertEqual(
            catalogErrorText(code: 403, serverMessage: "Requests are off.", network: false),
            "Requests are off."
        )
        XCTAssertEqual(
            catalogErrorText(code: 502, serverMessage: "Server returned 502", network: false),
            "Something went wrong. Try again."
        )
        XCTAssertEqual(
            catalogErrorText(code: 500, serverMessage: "HTTP 500", network: false),
            "Something went wrong. Try again."
        )
        let human = "Search is longer than 200 characters (asked 201)."
        XCTAssertEqual(catalogErrorText(code: 400, serverMessage: human, network: false), human)

        let wrapped = catalogError(from: HostedException(message: "Server returned 502", code: 502))
        XCTAssertEqual(wrapped.reason, .http(502))
        XCTAssertEqual(wrapped.message, "Something went wrong. Try again.")
        XCTAssertNil(wrapped.message.range(of: #"Server returned|HTTP \d+"#, options: .regularExpression))
        let offline = catalogError(from: URLError(.notConnectedToInternet))
        XCTAssertEqual(offline.reason, .network)
        XCTAssertFalse(offline.message.localizedCaseInsensitiveContains("http"))
    }

    func testSearchTabsAndAddBookOptions() {
        XCTAssertEqual(SearchTab.catalog.title, "Catalog")
        XCTAssertNil(librarySearchType(.catalog))
        XCTAssertEqual(librarySearchType(.books), "books")
        XCTAssertEqual(searchTabs(hosted: false, canRequest: true), [.books, .posts])
        XCTAssertEqual(searchTabs(hosted: false, canRequest: false), [.books, .posts])
        XCTAssertEqual(searchTabs(hosted: true, canRequest: false), [.accounts, .books, .posts])
        XCTAssertEqual(searchTabs(hosted: true, canRequest: true), [.accounts, .books, .posts, .catalog])
        XCTAssertEqual(addBookOptions(canRequest: false).map(\.title), ["Upload EPUB"])
        XCTAssertEqual(addBookOptions(canRequest: true).map(\.title), ["Upload EPUB", "Browse catalog"])
    }
}

final class CatalogDecodeTests: XCTestCase {
    func testFlattenedRequestDetailDecodesNullsAndAMissingServerCopy() throws {
        let detail = try JSONFiles.decoder.decode(WorkDetail.self, from: Data(flattenedWork.utf8))
        XCTAssertEqual(detail.workKey, "/works/OL1W")
        XCTAssertNil(detail.info)
        XCTAssertNil(detail.status.onServer)
        let request = try XCTUnwrap(detail.status.request)
        XCTAssertEqual(request.request.id, "req-1")
        XCTAssertEqual(request.request.workKey, "/works/OL1W")
        XCTAssertNil(request.request.year)
        XCTAssertNil(request.request.coverId)
        XCTAssertNil(request.request.error)
        XCTAssertNil(request.request.bookId)
        XCTAssertNil(request.request.follows)
        XCTAssertEqual(request.attempts, 2)
        XCTAssertEqual(request.maxAttempts, 4)
        XCTAssertNil(request.nextCheckAt)
        XCTAssertNil(request.releaseTitle)
        XCTAssertNil(request.releaseSize)
        XCTAssertEqual(request.phase, "searching")
        XCTAssertEqual(detail.status.active, 1)
        XCTAssertEqual(detail.status.cap, 5)
        XCTAssertTrue(detail.status.otherRequests.isEmpty)
    }

    func testCatalogPageDecodesAMissingServerCopy() throws {
        let page = try JSONFiles.decoder.decode(CatalogPage.self, from: Data(pageJSON.utf8))
        XCTAssertEqual(page.items.map(\.title), ["Dune", "The Left Hand of Darkness"])
        XCTAssertNil(page.nextPage)
        XCTAssertEqual(page.items[0].request?.id, "req-1")
        XCTAssertNil(page.items[0].request?.error)
        XCTAssertNil(page.items[0].onServer)
        XCTAssertNil(page.items[1].request)
        XCTAssertNil(page.items[1].onServer)
        XCTAssertNil(page.items[1].year)
        XCTAssertNil(page.items[1].coverId)
    }

    func testLibrarySearchRefusesACatalogPage() {
        let data = Data(#"{"type":"catalog","items":[],"nextCursor":null}"#.utf8)
        XCTAssertThrowsError(try JSONFiles.decoder.decode(SearchPage.self, from: data))
    }
}

@MainActor
final class CatalogModelTests: XCTestCase {
    private var fake = FakeCatalog()

    override func tearDown() {
        fake.finish()
        super.tearDown()
    }

    func testDebounceKeepsOnlyTheLatestQuery() async {
        fake = FakeCatalog()
        let feed = CatalogFeed(service: fake, debounceNanos: 200_000_000)
        feed.show()
        await waitPages(1)
        XCTAssertEqual(fake.pageLog.first?.query, nil)
        feed.setQuery("du")
        feed.setQuery("dune")
        await waitPages(2)
        XCTAssertEqual(fake.pageLog.map(\.query), [nil, "dune"])
    }

    func testShortQueriesNeverSearch() async {
        fake = FakeCatalog()
        fake.pageResult = .success(CatalogPage(items: [work(title: "Dune")], nextPage: nil))
        let feed = CatalogFeed(service: fake, debounceNanos: 0)
        feed.show()
        await waitPages(1)
        guard !fake.pageLog.isEmpty else { return }
        await feed.loadTask?.value
        feed.setQuery("a")
        feed.setQuery("📚")
        feed.setQuery(" ")
        try? await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(fake.pageLog.count, 1)
        XCTAssertEqual(fake.pageLog.first?.query, nil)
        XCTAssertEqual(feed.items.map(\.title), ["Dune"])

        feed.setQuery("📚📚")
        await waitPages(2)
        await feed.loadTask?.value
        XCTAssertEqual(fake.pageLog.map(\.query), [nil, "📚📚"])
        XCTAssertFalse(fake.pageLog.contains { $0.query == "a" || $0.query == "📚" })
    }

    func testStalePageDoesNotReplaceTheLatestQuery() async {
        fake = FakeCatalog()
        fake.holdPages = true
        let feed = CatalogFeed(service: fake, debounceNanos: 0)
        feed.setQuery("dune")
        feed.show()
        await waitPages(1)
        feed.setQuery("foundation")
        await waitPages(2)
        guard fake.pageLog.count >= 2 else { return }
        fake.resumePage(.success(CatalogPage(items: [work(title: "Dune")], nextPage: nil)))
        fake.resumePage(.success(CatalogPage(items: [work(title: "Foundation", key: "/works/OL2W")], nextPage: nil)))
        await feed.loadTask?.value
        XCTAssertEqual(feed.items.map(\.title), ["Foundation"])
    }

    func testSinglePageTrendingMemoRevalidatesAndAFailureKeepsIt() async {
        fake = FakeCatalog()
        fake.pageResult = .success(CatalogPage(items: [work(title: "Dune")], nextPage: nil))
        let feed = CatalogFeed(service: fake, debounceNanos: 0)
        feed.show()
        await waitPages(1)
        await feed.loadTask?.value
        XCTAssertEqual(feed.items.map(\.title), ["Dune"])

        feed.hide()
        fake.pageResult = .failure(HostedException(message: "Server returned 502", code: 502))
        feed.show()
        await waitPages(2)
        await feed.loadTask?.value
        XCTAssertEqual(feed.items.map(\.title), ["Dune"])
        XCTAssertNil(feed.failure)
        XCTAssertEqual(fake.pageLog.map(\.page), [1, 1])
    }

    func testMultiPageMemoDoesNotRefetch() async {
        fake = FakeCatalog()
        fake.pageHandler = { _, page in
            if page == 1 {
                return CatalogPage(items: [work(title: "Dune")], nextPage: 2)
            }
            return CatalogPage(items: [work(title: "Left", key: "/works/OL2W")], nextPage: nil)
        }
        let feed = CatalogFeed(service: fake, debounceNanos: 0)
        feed.show()
        await waitPages(1)
        await feed.loadTask?.value
        feed.loadMore()
        await waitPages(2)
        await feed.loadTask?.value
        XCTAssertEqual(feed.items.map(\.title), ["Dune", "Left"])

        feed.hide()
        let extra = expectation(description: "multi-page memo does not refetch")
        extra.isInverted = true
        let current = fake
        fake.onPage = { [weak current] in
            current?.onPage = nil
            extra.fulfill()
        }
        feed.show()
        await fulfillment(of: [extra], timeout: 0.2)
        fake.onPage = nil
        XCTAssertEqual(fake.pageLog.count, 2)
        XCTAssertEqual(feed.items.map(\.title), ["Dune", "Left"])
    }

    func testUserFacingFailureReplacesResultsWithHumanText() async {
        fake = FakeCatalog()
        fake.pageResult = .success(CatalogPage(items: [work(title: "Dune")], nextPage: nil))
        let feed = CatalogFeed(service: fake, debounceNanos: 0)
        feed.setQuery("dune")
        feed.show()
        await waitPages(1)
        await feed.loadTask?.value
        XCTAssertEqual(feed.items.map(\.title), ["Dune"])

        fake.pageResult = .failure(HostedException(message: "Server returned 502", code: 502))
        feed.setQuery("foundation")
        await waitPages(2)
        await feed.loadTask?.value
        XCTAssertEqual(feed.failure, "Something went wrong. Try again.")
        XCTAssertTrue(feed.items.isEmpty)
        XCTAssertNil(feed.failure?.range(of: #"Server returned|HTTP \d+"#, options: .regularExpression))
    }

    func testNoteSessionDropsAStaleToken() async {
        fake = FakeCatalog()
        fake.token = "a"
        fake.holdPages = true
        let feed = CatalogFeed(service: fake, debounceNanos: 0)
        feed.show()
        await waitPages(1)
        fake.token = "b"
        feed.noteSession("b")
        await waitPages(2)
        guard fake.pageLog.count >= 2 else { return }
        fake.resumePage(.success(CatalogPage(items: [work(title: "Stale")], nextPage: nil)))
        fake.resumePage(.success(CatalogPage(items: [work(title: "Fresh", key: "/works/OL2W")], nextPage: nil)))
        await feed.loadTask?.value
        XCTAssertEqual(feed.items.map(\.title), ["Fresh"])

        feed.noteSession(nil)
        XCTAssertEqual(feed.phase, .idle)
        XCTAssertTrue(feed.items.isEmpty)
    }

    func testRowRequestWritesTheOverlayAndImportsOnce() async {
        fake = FakeCatalog()
        var imports = 0
        let feed = CatalogFeed(service: fake, onImported: { imports += 1 }, debounceNanos: 0)
        fake.requestResult = .success(bookRequest(state: "imported", bookId: "book-1"))
        let requested = await feed.act(work())
        XCTAssertEqual(requested, .done)
        XCTAssertEqual(imports, 1)
        XCTAssertEqual(feed.shown(work())?.bookId, "book-1")

        let again = await feed.act(work())
        XCTAssertEqual(again, .openBook("book-1"))
        XCTAssertEqual(imports, 1)
        XCTAssertEqual(fake.requestLog.count, 1)
    }

    func testOpenBookDoesNotRequest() async {
        fake = FakeCatalog()
        let feed = CatalogFeed(service: fake, debounceNanos: 0)
        let outcome = await feed.act(work(onServer: heldCopy()))
        XCTAssertEqual(outcome, .openBook("held-book"))
        XCTAssertTrue(fake.requestLog.isEmpty)
        XCTAssertTrue(fake.retryLog.isEmpty)
    }

    func testSheetPollsWhileLiveAndStops() async {
        fake = FakeCatalog()
        fake.holdWork = true
        let model = sheet(poll: 20_000_000)
        defer { model.stop() }
        await waitWorks(1)
        fake.resumeWork(.success(workDetail(phase: "searching")))
        await waitWorks(2)
        model.stop()
        let stoppedAt = fake.workLog.count
        fake.resumeWork(.success(workDetail(phase: "searching")))
        await Task.yield()
        try? await Task.sleep(nanoseconds: 120_000_000)
        XCTAssertEqual(fake.workLog.count, stoppedAt)
    }

    func testSheetStopsOn401403And404() async {
        for reason in [CatalogErrorReason.unauthorized, .forbidden, .notFound] {
            fake.finish()
            fake = FakeCatalog()
            var pulls = 0
            fake.workHandler = {
                pulls += 1
                if pulls == 1 { return workDetail(phase: "searching") }
                throw CatalogError(reason: reason, message: "Stop.")
            }
            let model = sheet(poll: 30_000_000)
            defer { model.stop() }
            await waitWorks(2)
            try? await Task.sleep(nanoseconds: 150_000_000)
            XCTAssertEqual(fake.workLog.count, 2, "\(reason)")
        }
    }

    func testNetworkErrorPollsAgain() async {
        fake = FakeCatalog()
        var pulls = 0
        fake.workHandler = {
            pulls += 1
            if pulls < 3 { throw URLError(.cannotConnectToHost) }
            return workDetail(phase: "searching")
        }
        let model = sheet(poll: 20_000_000)
        defer { model.stop() }
        await waitWorks(3)
        XCTAssertNotNil(model.detail)
    }

    func testOptimisticCancelShowsBeforeTheServerReturns() async {
        fake = FakeCatalog()
        fake.workResult = .success(workDetail(phase: "searching"))
        var states: [String] = []
        let model = sheet(poll: 5_000_000_000, onChange: { states.append($0?.state ?? "nil") })
        defer { model.stop() }
        await waitWorks(1)
        await Task.yield()
        if case .live = model.status {} else {
            return XCTFail("expected a live status, got \(model.status)")
        }
        fake.holdCancel = true
        let task = Task { await model.cancel() }
        await waitCancels(1)
        XCTAssertEqual(states.last, "canceled")
        if case .canceled = model.status {} else {
            XCTFail("expected canceled before the server returned, got \(model.status)")
        }
        fake.resumeCancel()
        await task.value
        XCTAssertEqual(fake.cancelLog, ["req-1"])
    }

    func testRetryRefreshesTheWork() async {
        fake = FakeCatalog()
        fake.workResult = .success(workDetail(phase: "failed", state: "failed", error: "No EPUB"))
        let model = sheet(poll: 5_000_000_000)
        defer { model.stop() }
        await waitWorks(1)
        await Task.yield()
        let pulls = fake.workLog.count
        fake.retryResult = .success(bookRequest(state: "requested", error: "No EPUB"))
        await model.retry()
        await waitWorks(pulls + 1)
        XCTAssertEqual(fake.retryLog, ["req-1"])
        XCTAssertGreaterThan(fake.workLog.count, pulls)
    }

    func testImportedFiresOnceOnTransitionAndNotWhenAlreadyImported() async {
        fake = FakeCatalog()
        var imports = 0
        fake.workResult = .success(workDetail(phase: "imported", state: "imported", bookId: "book-1"))
        let opened = sheet(poll: 30_000_000, onImported: { imports += 1 })
        defer { opened.stop() }
        await waitWorks(1)
        await Task.yield()
        try? await Task.sleep(nanoseconds: 120_000_000)
        XCTAssertEqual(imports, 0)
        XCTAssertEqual(fake.workLog.count, 1)
        opened.stop()

        fake.finish()
        fake = FakeCatalog()
        var pulls = 0
        fake.workHandler = {
            pulls += 1
            if pulls == 1 { return workDetail(info: workInfo()) }
            return workDetail(phase: "imported", state: "imported", bookId: "book-1", info: workInfo())
        }
        imports = 0
        let model = sheet(poll: 20_000_000, onImported: { imports += 1 })
        defer { model.stop() }
        await waitWorks(1)
        await Task.yield()
        fake.requestResult = .success(bookRequest(state: "imported", bookId: "book-1"))
        await model.request()
        XCTAssertEqual(imports, 1)
        await waitWorks(2)
        await Task.yield()
        XCTAssertEqual(imports, 1)

        model.reload()
        await waitWorks(3)
        XCTAssertEqual(imports, 1)
    }

    func testCapAndActingBlockAnotherRequest() async {
        fake = FakeCatalog()
        fake.workResult = .success(workDetail(active: 5, cap: 5, accepted: false))
        let capped = sheet(poll: 5_000_000_000)
        defer { capped.stop() }
        await waitWorks(1)
        await Task.yield()
        if case .requestable(let note, let edition) = capped.status {
            XCTAssertEqual(note, "You have 5 requests in progress")
            XCTAssertEqual(edition, "No English edition listed on Open Library")
        } else {
            XCTFail("expected a requestable cap, got \(capped.status)")
        }
        await capped.request()
        XCTAssertTrue(fake.requestLog.isEmpty)

        fake.workResult = .success(workDetail(phase: "failed", state: "failed", error: "x", active: 5, cap: 5))
        capped.reload()
        await waitWorks(2)
        await Task.yield()
        await capped.retry()
        XCTAssertTrue(fake.retryLog.isEmpty)

        fake.finish()
        fake = FakeCatalog()
        fake.workResult = .success(workDetail(active: 0, cap: 5))
        fake.holdRequest = true
        let model = sheet(poll: 5_000_000_000)
        defer { model.stop() }
        await waitWorks(1)
        await Task.yield()
        let task = Task { await model.request() }
        await waitRequests(1)
        await model.request()
        XCTAssertEqual(fake.requestLog.count, 1)
        XCTAssertTrue(model.acting)
        fake.resumeRequest(.success(bookRequest()))
        await task.value
    }

    func testLiveStatusHidesTheCapAndAlsoRequestedIgnoresTheOwner() async {
        fake = FakeCatalog()
        fake.workResult = .success(workDetail(
            phase: "searching",
            active: 5,
            cap: 5,
            others: ["lin", "ada", "noor", "grace"],
            onServer: heldCopy()
        ))
        let model = sheet(poll: 5_000_000_000)
        defer { model.stop() }
        await waitWorks(1)
        await Task.yield()
        if case .live(let phase, let line, let progress) = model.status {
            XCTAssertEqual(phase, .searching)
            XCTAssertEqual(line, "Looking for a release…")
            XCTAssertNil(progress)
        } else {
            XCTFail("cap note leaked into \(model.status)")
        }
        XCTAssertEqual(model.alsoRequested, "Also requested by @lin, @ada, @noor +1")
        XCTAssertFalse(model.alsoRequested?.contains("secret") ?? true)
    }

    func testCatalogTabDoesNotCallLibrarySearch() async {
        fake = FakeCatalog()
        fake.access = .on
        fake.token = "token-a"
        let remote = RecordingSearch()
        let feed = CatalogFeed(service: fake, debounceNanos: 0)
        let model = SearchViewModel(
            library: QuietLibrary(),
            remote: remote,
            catalog: feed,
            canRequest: true,
            debounceNanos: 0
        )
        model.select(.catalog)
        model.setQuery("dune")
        await waitPages(1)
        await feed.loadTask?.value
        XCTAssertTrue(remote.calls.isEmpty)
        XCTAssertEqual(fake.pageLog.map(\.query), ["dune"])
        XCTAssertEqual(model.tab, .catalog)
    }

    func testSavedCatalogTabWaitsForAccess() {
        let defaults = UserDefaults(suiteName: "mewo-catalog-tabs-\(UUID().uuidString)")!
        defaults.set(SearchTab.catalog.rawValue, forKey: searchTabKey)
        let remote = RecordingSearch()
        let waiting = SearchViewModel(
            library: QuietLibrary(),
            remote: remote,
            defaults: defaults,
            canRequest: false
        )
        XCTAssertEqual(waiting.tab, .accounts)
        waiting.noteCatalogAccess(.unknown)
        XCTAssertEqual(waiting.tab, .accounts)
        waiting.noteCatalogAccess(.on)
        XCTAssertEqual(waiting.tab, .catalog)

        let allowed = SearchViewModel(
            library: QuietLibrary(),
            remote: remote,
            defaults: defaults,
            canRequest: true
        )
        XCTAssertEqual(allowed.tab, .catalog)
        allowed.noteCatalogAccess(.off)
        XCTAssertEqual(allowed.tab, .accounts)

        let local = SearchViewModel(library: QuietLibrary(), defaults: defaults)
        XCTAssertEqual(local.tab, .books)
        local.select(.catalog)
        XCTAssertEqual(local.tab, .books)
    }

    private func sheet(
        poll: UInt64,
        onChange: @escaping (BookRequest?) -> Void = { _ in },
        onImported: @escaping () -> Void = {}
    ) -> CatalogSheetModel {
        CatalogSheetModel(
            work: work(),
            request: nil,
            service: fake,
            pollNanos: poll,
            onChange: onChange,
            onImported: onImported
        )
    }

    private func waitPages(_ count: Int) async {
        if fake.pageLog.count >= count { return }
        let done = expectation(description: "catalog pages \(count)")
        fake.onPage = {
            if self.fake.pageLog.count >= count {
                self.fake.onPage = nil
                done.fulfill()
            }
        }
        await fulfillment(of: [done], timeout: 2)
    }

    private func waitWorks(_ count: Int) async {
        if fake.workLog.count >= count { return }
        let done = expectation(description: "catalog works \(count)")
        fake.onWork = {
            if self.fake.workLog.count >= count {
                self.fake.onWork = nil
                done.fulfill()
            }
        }
        await fulfillment(of: [done], timeout: 2)
    }

    private func waitCancels(_ count: Int) async {
        if fake.cancelLog.count >= count { return }
        let done = expectation(description: "cancels \(count)")
        fake.onCancel = {
            if self.fake.cancelLog.count >= count {
                self.fake.onCancel = nil
                done.fulfill()
            }
        }
        await fulfillment(of: [done], timeout: 2)
    }

    private func waitRequests(_ count: Int) async {
        if fake.requestLog.count >= count { return }
        let done = expectation(description: "requests \(count)")
        fake.onRequest = {
            if self.fake.requestLog.count >= count {
                self.fake.onRequest = nil
                done.fulfill()
            }
        }
        await fulfillment(of: [done], timeout: 2)
    }
}

@MainActor
final class CatalogHTTPTests: XCTestCase {
    private var backend: BackendStore?
    private var previousKind: BackendKind?

    override func tearDown() {
        if let backend, let previousKind {
            backend.setKind(previousKind)
        }
        backend = nil
        previousKind = nil
        super.tearDown()
    }

    func testLocalAndSignedOutStayOffWithoutARequest() async {
        CatalogHTTP.reset()
        let service = makeService(hosted: false, signedIn: false)
        await service.refreshAccess()
        await Task.yield()
        XCTAssertEqual(service.access, .off)
        XCTAssertTrue(CatalogHTTP.calls.isEmpty)

        CatalogHTTP.reset()
        let signedOut = makeService(hosted: true, signedIn: false)
        await signedOut.refreshAccess()
        await Task.yield()
        XCTAssertEqual(signedOut.access, .off)
        XCTAssertTrue(CatalogHTTP.calls.isEmpty)
    }

    func testMeTurnsCatalogOnOffAndA403HidesIt() async {
        CatalogHTTP.reset()
        CatalogHTTP.reply("/v1/me", status: 200, body: meJSON(canRequest: true))
        let service = makeService(hosted: true, signedIn: true)
        await settle(service)
        XCTAssertEqual(service.access, .on)

        CatalogHTTP.reply("/v1/me", status: 200, body: meJSON(canRequest: false))
        await service.refreshAccess()
        XCTAssertEqual(service.access, .off)

        CatalogHTTP.reply("/v1/me", status: 200, body: meJSON(canRequest: true))
        await service.refreshAccess()
        XCTAssertEqual(service.access, .on)
        CatalogHTTP.reply("/v1/me", status: 500, body: Data())
        await service.refreshAccess()
        XCTAssertEqual(service.access, .on, "a blip keeps the last answer")

        CatalogHTTP.reply("/v1/me", status: 403, body: Data(#"{"error":"Requests are off."}"#.utf8))
        await service.refreshAccess()
        XCTAssertEqual(service.access, .off)
    }

    func test401SignsOut() async {
        CatalogHTTP.reset()
        CatalogHTTP.reply("/v1/me", status: 401, body: Data(#"{"error":"Sign in again."}"#.utf8))
        let sessions = sessionStore(signedIn: true)
        let service = HostedCatalogService(sessions: sessions, client: client(), backend: useBackend(hosted: true))
        await settle(service)
        XCTAssertEqual(service.access, .off)
        XCTAssertNil(sessions.session.token)
        XCTAssertFalse(CatalogHTTP.calls.isEmpty)
        XCTAssertTrue(CatalogHTTP.calls.allSatisfy { $0.url.contains("/v1/me") })
    }

    func testPageOmitsShortQueriesAndMapsErrors() async throws {
        CatalogHTTP.reset()
        CatalogHTTP.reply("/v1/me", status: 200, body: meJSON(canRequest: true))
        CatalogHTTP.reply("/v1/catalog/search", status: 200, body: Data(#"{"items":[],"nextPage":null}"#.utf8))
        let service = makeService(hosted: true, signedIn: true)
        await settle(service)

        _ = try await service.page(query: "a", page: 1)
        _ = try await service.page(query: "📚", page: 1)
        _ = try await service.page(query: "  dune  ", page: 2)
        let searches = CatalogHTTP.calls.filter { $0.url.contains("/v1/catalog/search") }
        XCTAssertEqual(searches.count, 3)
        XCTAssertNil(queryValue(searches[0].url, "q"))
        XCTAssertNil(queryValue(searches[1].url, "q"))
        XCTAssertEqual(queryValue(searches[2].url, "q"), "dune")
        XCTAssertEqual(queryValue(searches[2].url, "page"), "2")

        CatalogHTTP.reply("/v1/catalog/search", status: 403, body: Data(#"{"error":"Requests are off."}"#.utf8))
        do {
            _ = try await service.page(query: "dune", page: 1)
            XCTFail("expected requests off")
        } catch let error as CatalogError {
            XCTAssertEqual(error.reason, .forbidden)
            XCTAssertEqual(error.message, "Requests are off.")
            XCTAssertNil(error.message.range(of: #"Server returned|HTTP \d+"#, options: .regularExpression))
        } catch {
            throw error
        }
        XCTAssertEqual(service.access, .off)

        CatalogHTTP.reply("/v1/me", status: 200, body: meJSON(canRequest: true))
        await service.refreshAccess()
        CatalogHTTP.reply("/v1/catalog/search", status: 502, body: Data())
        do {
            _ = try await service.page(query: "dune", page: 1)
            XCTFail("expected a human error")
        } catch let error as CatalogError {
            XCTAssertEqual(error.message, "Something went wrong. Try again.")
            XCTAssertNil(error.message.range(of: #"Server returned|HTTP \d+"#, options: .regularExpression))
        } catch {
            throw error
        }
        XCTAssertEqual(service.access, .on)
    }

    func testWorkCancelAndRetryUseTheCatalogRoutes() async throws {
        CatalogHTTP.reset()
        CatalogHTTP.reply("/v1/me", status: 200, body: meJSON(canRequest: true))
        CatalogHTTP.reply("/v1/catalog/works/OL1W", status: 200, body: Data(flattenedWork.utf8))
        CatalogHTTP.reply("/v1/requests/req-1", status: 204, body: Data())
        CatalogHTTP.reply("/v1/requests/req-1/retry", status: 200, body: Data(bookJSON.utf8))
        let service = makeService(hosted: true, signedIn: true)
        await settle(service)

        let detail = try await service.work(workKey: "/works/OL1W")
        XCTAssertEqual(detail.status.request?.phase, "searching")
        XCTAssertNil(detail.status.onServer)
        try await service.cancel(id: "req-1")
        let retried = try await service.retry(id: "req-1")
        XCTAssertEqual(retried.id, "req-1")
        XCTAssertTrue(CatalogHTTP.calls.contains { $0.method == "DELETE" && $0.url.contains("/v1/requests/req-1") })
        XCTAssertTrue(CatalogHTTP.calls.contains { $0.method == "POST" && $0.url.contains("/v1/requests/req-1/retry") })
    }

    func testCatalogIsNotSentToLibrarySearch() async {
        CatalogHTTP.reset()
        let http = client()
        let session = HostedSession(baseUrl: "http://catalog.test", username: "ada", token: "token")
        do {
            _ = try await http.search(session, query: "dune", type: "catalog", cursor: nil)
            XCTFail("expected catalog to stay off /v1/search")
        } catch let error as CatalogError {
            XCTAssertEqual(error.reason, .notSearch)
        } catch {
            XCTFail("unexpected \(error)")
        }
        let remote = HostedRemoteSearch(sessions: sessionStore(signedIn: true), client: http)
        do {
            _ = try await remote.search(query: "dune", type: .catalog, cursor: nil)
            XCTFail("expected the remote to refuse catalog")
        } catch let error as CatalogError {
            XCTAssertEqual(error.reason, .notSearch)
        } catch {
            XCTFail("unexpected \(error)")
        }
        XCTAssertTrue(CatalogHTTP.calls.isEmpty)
    }

    private func settle(_ service: HostedCatalogService) async {
        if service.access != .unknown { return }
        for _ in 0..<40 {
            if service.access != .unknown { return }
            try? await Task.sleep(nanoseconds: 25_000_000)
        }
        XCTFail("access stayed unknown")
    }

    private func makeService(hosted: Bool, signedIn: Bool) -> HostedCatalogService {
        HostedCatalogService(
            sessions: sessionStore(signedIn: signedIn),
            client: client(),
            backend: useBackend(hosted: hosted)
        )
    }

    private func useBackend(hosted: Bool) -> BackendStore {
        let store = backend ?? BackendStore()
        if backend == nil {
            previousKind = store.kind
            backend = store
        }
        store.setKind(hosted ? .hosted : .local)
        return store
    }

    private func sessionStore(signedIn: Bool) -> HostedSessionStore {
        let defaults = UserDefaults(suiteName: "mewo-catalog-http-\(UUID().uuidString)")!
        let sessions = HostedSessionStore(defaults: defaults, persistToken: false)
        sessions.setBaseUrl("http://catalog.test")
        if signedIn {
            sessions.setSignedIn(username: "ada", token: "token")
        }
        return sessions
    }

    private func client() -> HostedClient {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [CatalogHTTP.self]
        return HostedClient(session: URLSession(configuration: config))
    }

    private func queryValue(_ url: String, _ name: String) -> String? {
        URLComponents(string: url)?.queryItems?.first { $0.name == name }?.value
    }

    private func meJSON(canRequest: Bool) -> Data {
        Data(#"{"username":"ada","isAdmin":false,"canRequest":\#(canRequest)}"#.utf8)
    }
}

private func bookRequest(
    state: String = "requested",
    progress: Int = 0,
    error: String? = nil,
    bookId: String? = nil
) -> BookRequest {
    BookRequest(
        id: "req-1",
        workKey: "/works/OL893415W",
        title: "Dune",
        author: "Frank Herbert",
        year: 1965,
        coverId: nil,
        state: state,
        progress: progress,
        error: error,
        bookId: bookId,
        requester: "ada",
        createdAt: 0,
        updatedAt: 0,
        follows: nil
    )
}

private func heldCopy() -> ServerCopy {
    ServerCopy(bookId: "held-book", ownerUsername: "secret", mine: false, shared: false, held: true)
}

private func sharedCopy() -> ServerCopy {
    ServerCopy(bookId: "shared-book", ownerUsername: "alice", mine: false, shared: true, held: false)
}

private func work(
    title: String = "Dune",
    key: String = "/works/OL893415W",
    onServer: ServerCopy? = nil
) -> CatalogWork {
    CatalogWork(
        workKey: key,
        title: title,
        author: "Frank Herbert",
        year: 1965,
        coverId: nil,
        request: nil,
        onServer: onServer
    )
}

private func workInfo(accepted: Bool = true) -> WorkInfo {
    WorkInfo(
        canonicalKey: "/works/OL893415W",
        title: "Dune",
        subtitle: nil,
        authors: ["Frank Herbert"],
        year: 1965,
        coverId: nil,
        description: "Sand.",
        subjects: ["Science fiction"],
        awards: ["Hugo Award"],
        series: ["Dune"],
        editionCount: 400,
        pagesMedian: 608,
        languages: ["eng"],
        hasAcceptedEdition: accepted,
        rating: Rating(average: 4.3, count: 99)
    )
}

private func workDetail(
    phase: String? = nil,
    state: String = "requested",
    progress: Int = 0,
    error: String? = nil,
    bookId: String? = nil,
    active: Int = 0,
    cap: Int = 5,
    others: [String] = [],
    onServer: ServerCopy? = nil,
    info: WorkInfo? = nil,
    accepted: Bool? = nil
) -> WorkDetail {
    let request: RequestDetail?
    if let phase {
        request = RequestDetail(
            request: bookRequest(state: state, progress: progress, error: error, bookId: bookId),
            attempts: 1,
            maxAttempts: 4,
            nextCheckAt: nil,
            releaseTitle: nil,
            releaseSize: nil,
            phase: phase
        )
    } else {
        request = nil
    }
    let resolved = info ?? accepted.map { workInfo(accepted: $0) }
    let otherRequests = others.map { name in
        var copy = bookRequest()
        copy.id = name
        copy.requester = name
        return copy
    }
    return WorkDetail(
        workKey: "/works/OL893415W",
        info: resolved,
        infoError: nil,
        status: WorkStatus(
            request: request,
            onServer: onServer,
            active: active,
            cap: cap,
            otherRequests: otherRequests
        )
    )
}

private let flattenedWork = """
{
  "workKey": "/works/OL1W",
  "info": null,
  "status": {
    "request": {
      "id": "req-1",
      "workKey": "/works/OL1W",
      "title": "Dune",
      "author": "Frank Herbert",
      "year": null,
      "coverId": null,
      "state": "requested",
      "progress": 0,
      "error": null,
      "bookId": null,
      "requester": "ada",
      "createdAt": 10,
      "updatedAt": 11,
      "follows": null,
      "attempts": 2,
      "maxAttempts": 4,
      "nextCheckAt": null,
      "releaseTitle": null,
      "releaseSize": null,
      "phase": "searching"
    },
    "active": 1,
    "cap": 5,
    "otherRequests": []
  }
}
"""

private let pageJSON = """
{
  "items": [
    {
      "workKey": "/works/OL1W",
      "title": "Dune",
      "author": "Frank Herbert",
      "year": 1965,
      "coverId": null,
      "request": {
        "id": "req-1",
        "workKey": "/works/OL1W",
        "title": "Dune",
        "author": "Frank Herbert",
        "year": 1965,
        "coverId": null,
        "state": "requested",
        "progress": 0,
        "error": null,
        "bookId": null,
        "requester": "ada",
        "createdAt": 10,
        "updatedAt": 11
      }
    },
    {
      "workKey": "/works/OL2W",
      "title": "The Left Hand of Darkness",
      "author": "Ursula K. Le Guin",
      "year": null,
      "coverId": null,
      "request": null
    }
  ],
  "nextPage": null
}
"""

private let bookJSON = """
{
  "id": "req-1",
  "workKey": "/works/OL1W",
  "title": "Dune",
  "author": "Frank Herbert",
  "year": 1965,
  "coverId": null,
  "state": "requested",
  "progress": 0,
  "error": null,
  "bookId": null,
  "requester": "ada",
  "createdAt": 10,
  "updatedAt": 11
}
"""

@MainActor
private final class FakeCatalog: CatalogService {
    var access: CatalogAccess = .on
    var token: String? = "tok"
    var pageLog: [PageCall] = []
    var workLog: [String] = []
    var requestLog: [NewBookRequest] = []
    var cancelLog: [String] = []
    var retryLog: [String] = []
    var onPage: (() -> Void)?
    var onWork: (() -> Void)?
    var onCancel: (() -> Void)?
    var onRequest: (() -> Void)?
    var pageResult: Result<CatalogPage, Error> = .success(CatalogPage(items: [], nextPage: nil))
    var pageHandler: ((String?, Int) throws -> CatalogPage)?
    var holdPages = false
    var workResult: Result<WorkDetail, Error> = .success(workDetail())
    var workHandler: (() throws -> WorkDetail)?
    var holdWork = false
    var requestResult: Result<BookRequest, Error> = .failure(CatalogError(reason: .off, message: "Requests are off."))
    var holdRequest = false
    var holdCancel = false
    var retryResult: Result<BookRequest, Error> = .failure(CatalogError(reason: .off, message: "Requests are off."))
    struct PageCall: Equatable {
        var query: String?
        var page: Int
    }

    private var pageWaiters: [CheckedContinuation<CatalogPage, Error>] = []
    private var workWaiters: [CheckedContinuation<WorkDetail, Error>] = []
    private var requestWaiters: [CheckedContinuation<BookRequest, Error>] = []
    private var cancelWaiters: [CheckedContinuation<Void, Error>] = []

    func refreshAccess() async {}

    func page(query: String?, page: Int) async throws -> CatalogPage {
        pageLog.append(PageCall(query: query, page: page))
        onPage?()
        if holdPages {
            return try await withCheckedThrowingContinuation { pageWaiters.append($0) }
        }
        if let pageHandler { return try pageHandler(query, page) }
        return try pageResult.get()
    }

    func work(workKey: String) async throws -> WorkDetail {
        workLog.append(workKey)
        onWork?()
        if holdWork {
            return try await withCheckedThrowingContinuation { workWaiters.append($0) }
        }
        if let workHandler { return try workHandler() }
        return try workResult.get()
    }

    func request(_ body: NewBookRequest) async throws -> BookRequest {
        requestLog.append(body)
        onRequest?()
        if holdRequest {
            return try await withCheckedThrowingContinuation { requestWaiters.append($0) }
        }
        return try requestResult.get()
    }

    func cancel(id: String) async throws {
        cancelLog.append(id)
        onCancel?()
        if holdCancel {
            try await withCheckedThrowingContinuation { cancelWaiters.append($0) }
        }
    }

    func retry(id: String) async throws -> BookRequest {
        retryLog.append(id)
        return try retryResult.get()
    }

    func resumePage(_ result: Result<CatalogPage, Error>) {
        guard !pageWaiters.isEmpty else { return }
        resume(pageWaiters.removeFirst(), result)
    }

    func resumeWork(_ result: Result<WorkDetail, Error>) {
        guard !workWaiters.isEmpty else { return }
        resume(workWaiters.removeFirst(), result)
    }

    func resumeRequest(_ result: Result<BookRequest, Error>) {
        guard !requestWaiters.isEmpty else { return }
        resume(requestWaiters.removeFirst(), result)
    }

    func resumeCancel() {
        guard !cancelWaiters.isEmpty else { return }
        cancelWaiters.removeFirst().resume()
    }

    func finish() {
        pageWaiters.forEach { $0.resume(throwing: CancellationError()) }
        workWaiters.forEach { $0.resume(throwing: CancellationError()) }
        requestWaiters.forEach { $0.resume(throwing: CancellationError()) }
        cancelWaiters.forEach { $0.resume(throwing: CancellationError()) }
        pageWaiters.removeAll()
        workWaiters.removeAll()
        requestWaiters.removeAll()
        cancelWaiters.removeAll()
    }

    private func resume<T>(_ continuation: CheckedContinuation<T, Error>, _ result: Result<T, Error>) {
        switch result {
        case .success(let value): continuation.resume(returning: value)
        case .failure(let error): continuation.resume(throwing: error)
        }
    }
}

@MainActor
private final class QuietLibrary: LibraryStore {
    var books: [BookRecord] = []
    var loadState: LibraryLoad = .loaded(books: [], refreshing: false)
    func load() async throws {}
    func dismissLoadError() {}
    func importFromFile(_ file: URL) async throws -> BookRecord { throw CancellationError() }
    func importSample() async throws -> BookRecord { throw CancellationError() }
    func delete(id: String) async throws {}
    func book(id: String) -> BookRecord? { nil }
    func feed(id: String) async throws -> [FeedPost] { [] }
    func saveProgress(id: String, index: Int) async throws {}
    func likes(id: String) async throws -> Set<String> { [] }
    func toggleLike(id: String, postId: String) async throws -> Set<String> { [] }
    func setRepost(id: String, postId: String, on: Bool) async throws -> RepostState {
        RepostState(reposts: on ? 1 : 0, reposted: on)
    }
    func commentIndex(id: String) async throws -> CommentIndex { CommentIndex() }
    func comments(id: String, postId: String) async throws -> [PostComment] { [] }
    func addComment(id: String, postId: String, text: String) async throws -> [PostComment] { [] }
    func deleteComment(id: String, postId: String, commentId: String) async throws -> [PostComment] { [] }
    func search(query: String) async throws -> SearchResult { SearchResult() }
    func likedPosts() async throws -> [PostHit] { [] }
    func profileReplies(handle: String) async throws -> [ProfileReply] { [] }
    func coverFile(id: String) -> URL? { nil }
}

@MainActor
private final class RecordingSearch: RemoteSearch {
    var token: String? = "token-a"
    var calls: [(query: String, type: SearchTab)] = []

    func search(query: String, type: SearchTab, cursor: String?) async throws -> SearchPage {
        calls.append((query, type))
        return SearchPage(type: .books)
    }
}

private final class CatalogHTTP: URLProtocol, @unchecked Sendable {
    struct Hit: Equatable, Sendable {
        var method: String
        var url: String
    }

    struct Reply: Sendable {
        var status: Int
        var body: Data
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var replies: [String: Reply] = [:]
    nonisolated(unsafe) private static var hits: [Hit] = []

    static var calls: [Hit] {
        lock.lock()
        defer { lock.unlock() }
        return hits
    }

    static func reset() {
        lock.lock()
        replies = [:]
        hits = []
        lock.unlock()
    }

    static func reply(_ path: String, status: Int, body: Data) {
        lock.lock()
        replies[path] = Reply(status: status, body: body)
        lock.unlock()
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let url = request.url
        let path = url?.path ?? ""
        let hit = Hit(method: request.httpMethod ?? "GET", url: url?.absoluteString ?? "")
        Self.lock.lock()
        Self.hits.append(hit)
        let reply = Self.replies[path] ?? Reply(status: 500, body: Data())
        Self.lock.unlock()
        let response = HTTPURLResponse(
            url: url ?? URL(string: "http://catalog.test")!,
            statusCode: reply.status,
            httpVersion: "HTTP/1.1",
            headerFields: nil
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: reply.body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
