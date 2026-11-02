import XCTest
@testable import Mewo

@MainActor
final class SearchBrowseTests: XCTestCase {
    private let clock = BrowseClock()

    override func setUp() {
        super.setUp()
        clock.date = Date(timeIntervalSince1970: 1_700_000_000)
    }

    func testHeadingsAndEmptyCopyMatchTheWeb() {
        XCTAssertEqual(browseRevalidateSeconds, 60)
        XCTAssertEqual(searchHeading(.accounts), "Suggested accounts")
        XCTAssertEqual(searchHeading(.books), "Popular on this server")
        XCTAssertEqual(searchHeading(.posts), "Top posts")
        XCTAssertNil(searchHeading(.catalog))

        XCTAssertEqual(searchEmptyCopy(.accounts, .nothingYet, query: "  "), "It's just you here so far.")
        XCTAssertEqual(searchEmptyCopy(.books, .nothingYet, query: ""), "No books yet.")
        XCTAssertEqual(
            searchEmptyCopy(.posts, .nothingYet, query: ""),
            "No posts yet. Replies and reposts on books show up here."
        )
        XCTAssertEqual(searchEmptyCopy(.accounts, .noMatches, query: " dune "), "No accounts match “dune”")
        XCTAssertEqual(searchEmptyCopy(.books, .noMatches, query: "dune"), "No books match “dune”")
        XCTAssertEqual(searchEmptyCopy(.posts, .noMatches, query: "dune"), "No posts match “dune”")
        XCTAssertEqual(searchEmptyCopy(.accounts, .none, query: "dune"), "")

        XCTAssertEqual(
            searchErrorText(URLError(.notConnectedToInternet)),
            "Couldn't reach the server. Check your connection."
        )
        XCTAssertEqual(
            searchErrorText(URLError(.timedOut)),
            "Couldn't reach the server. Check your connection."
        )
        XCTAssertEqual(searchErrorText(HostedException(message: "nope", code: 500)), "nope")
    }

    func testBlankHostedQueryBrowsesAccountsWithoutDebounce() async {
        let (model, remote, _) = hosted()
        XCTAssertTrue(model.browsing)
        XCTAssertEqual(model.tab, .accounts)
        model.configure(remote: remote)
        model.noteSession(remote.token)
        model.refresh()
        await awaitCall(remote, 1)
        XCTAssertEqual(remote.calls.count, 1)
        XCTAssertEqual(remote.calls[0].query, "")
        XCTAssertEqual(remote.calls[0].type, .accounts)
        XCTAssertNil(remote.calls[0].cursor)
        remote.calls[0].continuation.resume(returning: accountPage("ada"))
        await model.searchTask?.value
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])
        XCTAssertFalse(model.searching)
        XCTAssertTrue(model.browsing)
        XCTAssertFalse(model.showNothingYet())
        XCTAssertFalse(model.showNoMatches())
    }

    func testLocalBlankMakesNoCall() async {
        let library = BrowseLibrary()
        let model = SearchViewModel(library: library, debounceNanos: 0, now: { self.clock.date })
        model.setQuery("   ")
        model.refresh()
        XCTAssertTrue(library.searches.isEmpty)
        XCTAssertFalse(model.browsing)
        XCTAssertFalse(model.showNothingYet())
        XCTAssertFalse(model.showNoMatches())
        model.setQuery("whale")
        await model.searchTask?.value
        XCTAssertEqual(library.searches, ["whale"])
    }

    func testMemoServesTheSecondVisitAndAFailedQuietRefreshKeepsTheRows() async {
        let (model, remote, _) = hosted()
        model.refresh()
        await awaitCall(remote, 1)
        remote.calls[0].continuation.resume(returning: accountPage("ada"))
        await model.searchTask?.value

        model.select(.books)
        await awaitCall(remote, 2)
        remote.calls[1].continuation.resume(returning: bookPage("dune"))
        await model.searchTask?.value

        await settleNoCall(remote) {
            model.select(.accounts)
        }
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])
        XCTAssertFalse(model.searching)

        clock.date = clock.date.addingTimeInterval(60)
        await settleNoCall(remote) {
            model.select(.books)
            model.select(.accounts)
        }

        clock.date = clock.date.addingTimeInterval(1)
        model.select(.books)
        await awaitCall(remote, 3)
        XCTAssertEqual(remote.calls[2].query, "")
        XCTAssertEqual(remote.calls[2].type, .books)
        XCTAssertEqual(model.visibleBooks().map(\.id), ["dune"])
        XCTAssertFalse(model.searching)
        XCTAssertNil(model.failure)
        remote.calls[2].continuation.resume(throwing: URLError(.networkConnectionLost))
        await model.searchTask?.value
        XCTAssertNil(model.failure)
        XCTAssertEqual(model.visibleBooks().map(\.id), ["dune"])
        XCTAssertFalse(model.searching)
    }

    func testClearingShowsTheBrowseMemoAndTypingKeepsItUntilThePageLands() async {
        let (model, remote, _) = hosted(debounceNanos: 0)
        model.refresh()
        await awaitCall(remote, 1)
        remote.calls[0].continuation.resume(returning: accountPage("ada"))
        await model.searchTask?.value

        model.setQuery("dune")
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])
        XCTAssertTrue(model.searching)
        XCTAssertFalse(model.browsing)
        await awaitCall(remote, 2)
        XCTAssertEqual(remote.calls[1].query, "dune")
        remote.calls[1].continuation.resume(returning: accountPage())
        await model.searchTask?.value
        XCTAssertTrue(model.visibleAccounts().isEmpty)
        XCTAssertTrue(model.showNoMatches())
        XCTAssertFalse(model.showNothingYet())

        let calls = remote.calls.count
        model.setQuery("")
        XCTAssertTrue(model.browsing)
        XCTAssertFalse(model.searching)
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])
        XCTAssertFalse(model.showNoMatches())
        XCTAssertFalse(model.showNothingYet())
        await settleNoCall(remote) {}
        XCTAssertEqual(remote.calls.count, calls)

        model.setQuery("   ")
        XCTAssertTrue(model.browsing)
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])
    }

    /// Typing, clearing, then typing again must not show the first query's rows under the second.
    func testTheNextQueryDoesNotShowRowsFromBeforeTheFieldWasCleared() async {
        let (model, remote, _) = hosted(debounceNanos: 0)
        model.select(.books)
        await awaitCall(remote, 1)
        remote.calls[0].continuation.resume(returning: bookPage("ada"))
        await model.searchTask?.value

        model.setQuery("dune")
        await awaitCall(remote, 2)
        remote.calls[1].continuation.resume(returning: bookPage("dune"))
        await model.searchTask?.value
        XCTAssertEqual(model.visibleBooks().map(\.id), ["dune"])

        model.setQuery("")
        XCTAssertEqual(model.visibleBooks().map(\.id), ["ada"])

        model.setQuery("a")
        XCTAssertTrue(model.searching)
        XCTAssertFalse(model.browsing)
        XCTAssertEqual(model.visibleBooks().map(\.id), ["ada"])
        await awaitCall(remote, 3)
        remote.calls[2].continuation.resume(throwing: URLError(.notConnectedToInternet))
        await model.searchTask?.value
        XCTAssertEqual(model.failure, "Couldn't reach the server. Check your connection.")
        XCTAssertEqual(model.visibleBooks().map(\.id), ["ada"])
    }

    /// A typed page that arrives after the field is cleared must not replace the browse page.
    func testATypedPageInFlightIsDroppedWhenTheFieldIsCleared() async {
        let (model, remote, _) = hosted(debounceNanos: 0)
        model.refresh()
        await awaitCall(remote, 1)
        remote.calls[0].continuation.resume(returning: accountPage("ada"))
        await model.searchTask?.value

        model.setQuery("dune")
        await awaitCall(remote, 2)
        let typed = model.searchTask
        model.setQuery("")
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])
        XCTAssertTrue(model.browsing)
        remote.calls[1].continuation.resume(returning: accountPage("dune"))
        await typed?.value

        model.setQuery("a")
        XCTAssertTrue(model.searching)
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])
        await awaitCall(remote, 3)
        XCTAssertEqual(remote.calls[2].query, "a")
        remote.calls[2].continuation.resume(returning: accountPage("amy"))
        await model.searchTask?.value
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["amy"])
    }

    func testNewSessionDropsTheMemoAndIgnoresTheOldPage() async {
        let (model, remote, _) = hosted()
        model.refresh()
        await awaitCall(remote, 1)
        remote.token = "token-b"
        model.noteSession("token-b")
        await awaitCall(remote, 2)
        remote.calls[0].continuation.resume(returning: accountPage("old"))
        remote.calls[1].continuation.resume(returning: accountPage("new"))
        await model.searchTask?.value
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["new"])
        XCTAssertEqual(remote.calls[1].query, "")
        XCTAssertEqual(remote.calls[1].type, .accounts)

        let kept = remote.calls.count
        model.configure(remote: remote)
        await settleNoCall(remote) {}
        XCTAssertEqual(remote.calls.count, kept)
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["new"])

        model.configure(remote: nil)
        XCTAssertFalse(model.hosted)
        XCTAssertFalse(model.browsing)
        XCTAssertTrue(model.visibleAccounts().isEmpty)
        XCTAssertEqual(model.tab, .books)
    }

    func testNoteSessionWithABlankQueryBrowses() async {
        let remote = FakeBrowseSearch()
        let model = SearchViewModel(
            library: BrowseLibrary(),
            remote: remote,
            debounceNanos: 60_000_000_000,
            now: { self.clock.date }
        )
        XCTAssertTrue(remote.calls.isEmpty)
        remote.token = "token-a"
        model.noteSession("token-a")
        await awaitCall(remote, 1)
        XCTAssertEqual(remote.calls[0].query, "")
        XCTAssertEqual(remote.calls[0].type, .accounts)
        XCTAssertTrue(model.browsing)
    }

    func testColdFailureRetryAndStaleRefresh() async {
        let (model, remote, _) = hosted()
        model.refresh()
        await awaitCall(remote, 1)
        remote.calls[0].continuation.resume(throwing: URLError(.notConnectedToInternet))
        await model.searchTask?.value
        XCTAssertEqual(model.failure, "Couldn't reach the server. Check your connection.")
        XCTAssertTrue(model.visibleAccounts().isEmpty)
        XCTAssertFalse(model.showNothingYet())
        XCTAssertFalse(model.showNoMatches())

        model.retry()
        await awaitCall(remote, 2)
        XCTAssertNil(model.failure)
        XCTAssertEqual(remote.calls[1].query, "")
        remote.calls[1].continuation.resume(throwing: HostedException(message: "nope", code: 500))
        await model.searchTask?.value
        XCTAssertEqual(model.failure, "nope")

        model.refresh()
        await awaitCall(remote, 3)
        XCTAssertNil(model.failure)
        remote.calls[2].continuation.resume(returning: accountPage("ada"))
        await model.searchTask?.value
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])

        clock.date = clock.date.addingTimeInterval(61)
        model.refresh()
        await awaitCall(remote, 4)
        XCTAssertEqual(remote.calls[3].query, "")
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])
        XCTAssertNil(model.failure)
        remote.calls[3].continuation.resume(returning: accountPage("bea"))
        await model.searchTask?.value
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["bea"])
    }

    func testEmptyKindTruthTable() async {
        let (model, remote, _) = hosted(debounceNanos: 0)
        model.refresh()
        await awaitCall(remote, 1)
        remote.calls[0].continuation.resume(returning: accountPage())
        await model.searchTask?.value
        XCTAssertTrue(model.showNothingYet())
        XCTAssertFalse(model.showNoMatches())

        model.select(.books)
        await awaitCall(remote, 2)
        remote.calls[1].continuation.resume(returning: bookPage("dune"))
        await model.searchTask?.value
        XCTAssertFalse(model.showNothingYet())
        XCTAssertFalse(model.showNoMatches())

        model.setQuery("dune")
        XCTAssertFalse(model.showNoMatches())
        XCTAssertFalse(model.showNothingYet())
        await awaitCall(remote, 3)
        remote.calls[2].continuation.resume(returning: bookPage())
        await model.searchTask?.value
        XCTAssertTrue(model.visibleBooks().isEmpty)
        XCTAssertTrue(model.showNoMatches())
        XCTAssertFalse(model.showNothingYet())

        model.setQuery("more")
        XCTAssertTrue(model.searching)
        XCTAssertFalse(model.showNoMatches())
        XCTAssertFalse(model.showNothingYet())
        await awaitCall(remote, 4)
        remote.calls[3].continuation.resume(returning: bookPage("more"))
        await model.searchTask?.value

        model.select(.catalog)
        XCTAssertFalse(model.showNothingYet())
        XCTAssertFalse(model.showNoMatches())
        await settleNoCall(remote) {
            model.setQuery("")
            model.refresh()
        }

        let local = SearchViewModel(library: BrowseLibrary(), now: { self.clock.date })
        XCTAssertFalse(local.showNothingYet())
        XCTAssertFalse(local.showNoMatches())
    }

    func testPostsBrowseLoadsSocialForEachRowAndAMemoHitLoadsAgain() async {
        let (model, remote, library) = hosted()
        model.select(.posts)
        await awaitCall(remote, 1)
        XCTAssertEqual(remote.calls[0].query, "")
        XCTAssertEqual(remote.calls[0].type, .posts)
        remote.calls[0].continuation.resume(returning: SearchPage(type: .posts, posts: [
            postHit("a", "1"),
            postHit("b", "2"),
        ]))
        await model.searchTask?.value
        await model.social(for: "a")?.pending?.value
        await model.social(for: "b")?.pending?.value
        XCTAssertEqual(Set(library.likeIds), ["a", "b"])

        let calls = remote.calls.count
        let likes = library.likeIds.count
        model.refresh()
        await model.social(for: "a")?.pending?.value
        await model.social(for: "b")?.pending?.value
        XCTAssertEqual(remote.calls.count, calls)
        XCTAssertGreaterThan(library.likeIds.count, likes)
        XCTAssertEqual(model.visiblePosts().map(\.id), ["a:1", "b:2"])
    }

    func testBlankSearchOmitsQ() async throws {
        BrowseHTTP.reset()
        let page = Data(#"{"type":"books","items":[],"nextCursor":null}"#.utf8)
        BrowseHTTP.reply("/v1/search", status: 200, body: page)
        let client = browseClient()
        let session = HostedSession(baseUrl: "http://browse.test", username: "ada", token: "token")

        _ = try await client.search(session, query: "", type: "books", cursor: nil)
        _ = try await client.search(session, query: "  ", type: "books", cursor: nil)
        _ = try await client.search(session, query: "dune", type: "books", cursor: "next")
        _ = try await client.search(session, query: "a b", type: "accounts", cursor: nil)

        let urls = BrowseHTTP.calls.map(\.url)
        XCTAssertEqual(urls.count, 4)
        XCTAssertNil(queryValue(urls[0], "q"))
        XCTAssertEqual(queryValue(urls[0], "type"), "books")
        XCTAssertNil(queryValue(urls[1], "q"))
        XCTAssertEqual(queryValue(urls[1], "type"), "books")
        XCTAssertEqual(queryValue(urls[2], "q"), "dune")
        XCTAssertEqual(queryValue(urls[2], "type"), "books")
        XCTAssertEqual(queryValue(urls[2], "cursor"), "next")
        XCTAssertEqual(queryValue(urls[3], "q"), "a b")
        XCTAssertEqual(queryValue(urls[3], "type"), "accounts")
        XCTAssertNil(queryValue(urls[3], "cursor"))
    }

    /// Browse-only cases keep a 60s debounce, so a blank query that slept would miss the wait.
    /// A typed query sleeps that long before `search`, so cases that type pass zero.
    private func hosted(
        token: String = "token-a",
        debounceNanos: UInt64 = 60_000_000_000
    ) -> (SearchViewModel, FakeBrowseSearch, BrowseLibrary) {
        let remote = FakeBrowseSearch()
        remote.token = token
        let library = BrowseLibrary()
        let model = SearchViewModel(
            library: library,
            remote: remote,
            debounceNanos: debounceNanos,
            now: { self.clock.date }
        )
        return (model, remote, library)
    }

    private func awaitCall(_ remote: FakeBrowseSearch, _ count: Int) async {
        if remote.calls.count >= count { return }
        let done = expectation(description: "search \(count)")
        remote.onCall = {
            if remote.calls.count >= count {
                remote.onCall = nil
                done.fulfill()
            }
        }
        await fulfillment(of: [done], timeout: 2)
    }

    /// Fails if `body` starts a search. Waits out a fast mistaken fetch; a real browse does not debounce.
    private func settleNoCall(_ remote: FakeBrowseSearch, _ body: () -> Void) async {
        let start = remote.calls.count
        let extra = expectation(description: "no search")
        extra.isInverted = true
        remote.onCall = {
            if remote.calls.count > start { extra.fulfill() }
        }
        body()
        await fulfillment(of: [extra], timeout: 0.25)
        remote.onCall = nil
        XCTAssertEqual(remote.calls.count, start)
    }

    private func accountPage(_ names: String...) -> SearchPage {
        SearchPage(type: .accounts, accounts: names.map { AccountRecord(username: $0) })
    }

    private func bookPage(_ ids: String...) -> SearchPage {
        SearchPage(type: .books, books: ids.map {
            BookRecord(id: $0, title: $0, author: "A", handle: "a", importedAt: 0)
        })
    }

    private func postHit(_ bookId: String, _ postId: String) -> PostHit {
        PostHit(
            book: BookRecord(id: bookId, title: bookId, author: "A", handle: "a", importedAt: 0),
            post: FeedPost(id: postId, kind: .body, text: "line", chapter: "One")
        )
    }

    private func browseClient() -> HostedClient {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [BrowseHTTP.self]
        return HostedClient(session: URLSession(configuration: config))
    }

    private func queryValue(_ url: String, _ name: String) -> String? {
        URLComponents(string: url)?.queryItems?.first { $0.name == name }?.value
    }
}

@MainActor
private final class BrowseClock {
    var date = Date(timeIntervalSince1970: 1_700_000_000)
}

@MainActor
private final class FakeBrowseSearch: RemoteSearch {
    var token: String?
    var calls: [Call] = []
    var onCall: (() -> Void)?

    struct Call {
        var query: String
        var type: SearchTab
        var cursor: String?
        var continuation: CheckedContinuation<SearchPage, Error>
    }

    func search(query: String, type: SearchTab, cursor: String?) async throws -> SearchPage {
        try await withCheckedThrowingContinuation { cont in
            calls.append(Call(query: query, type: type, cursor: cursor, continuation: cont))
            onCall?()
        }
    }
}

@MainActor
private final class BrowseLibrary: LibraryStore {
    var books: [BookRecord] = []
    var loadState: LibraryLoad = .loaded(books: [], refreshing: false)
    var searches: [String] = []
    var likeIds: [String] = []

    func search(query: String) async throws -> SearchResult {
        searches.append(query)
        return SearchResult()
    }

    func likes(id: String) async throws -> Set<String> {
        likeIds.append(id)
        return []
    }

    func load() async throws {}
    func dismissLoadError() {}
    func importFromFile(_ file: URL) async throws -> BookRecord { throw CancellationError() }
    func importSample() async throws -> BookRecord { throw CancellationError() }
    func delete(id: String) async throws {}
    func book(id: String) -> BookRecord? { nil }
    func feed(id: String) async throws -> [FeedPost] { [] }
    func saveProgress(id: String, index: Int) async throws {}
    func toggleLike(id: String, postId: String) async throws -> Set<String> { [] }
    func setRepost(id: String, postId: String, on: Bool) async throws -> RepostState {
        RepostState(reposts: on ? 1 : 0, reposted: on)
    }
    func commentIndex(id: String) async throws -> CommentIndex { CommentIndex() }
    func comments(id: String, postId: String) async throws -> [PostComment] { [] }
    func addComment(id: String, postId: String, text: String) async throws -> [PostComment] { [] }
    func deleteComment(id: String, postId: String, commentId: String) async throws -> [PostComment] { [] }
    func likedPosts() async throws -> [PostHit] { [] }
    func profileReplies(handle: String) async throws -> [ProfileReply] { [] }
    func coverFile(id: String) -> URL? { nil }
}

private final class BrowseHTTP: URLProtocol, @unchecked Sendable {
    struct Hit: Equatable, Sendable {
        var url: String
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var hits: [Hit] = []
    nonisolated(unsafe) private static var body = Data()
    nonisolated(unsafe) private static var status = 200

    static var calls: [Hit] {
        lock.lock()
        defer { lock.unlock() }
        return hits
    }

    static func reset() {
        lock.lock()
        hits = []
        body = Data()
        status = 200
        lock.unlock()
    }

    static func reply(_ path: String, status: Int, body: Data) {
        lock.lock()
        self.status = status
        self.body = body
        lock.unlock()
        _ = path
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let url = request.url
        Self.lock.lock()
        Self.hits.append(Hit(url: url?.absoluteString ?? ""))
        let status = Self.status
        let body = Self.body
        Self.lock.unlock()
        let response = HTTPURLResponse(
            url: url ?? URL(string: "http://browse.test")!,
            statusCode: status,
            httpVersion: "HTTP/1.1",
            headerFields: nil
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
