import XCTest
@testable import Mewo

final class SocialDecodeTests: XCTestCase {
    private let book = """
    {"id":"b1","title":"Emma","author":"Austen","handle":"austen","importedAt":0,"postCount":3,"progressIndex":0,"isSample":false,"mine":true,"shared":false,"ownerUsername":"ada"}
    """

    func testDecodesSearchPages() throws {
        let accounts = """
        {"type":"accounts","items":[{"username":"ada","mine":false}],"nextCursor":null}
        """.data(using: .utf8)!
        let page = try JSONFiles.decoder.decode(SearchPage.self, from: accounts)
        XCTAssertEqual(page.type, .accounts)
        XCTAssertEqual(page.accounts.map(\.username), ["ada"])
        XCTAssertNil(page.nextCursor)

        let books = """
        {"type":"books","items":[\(book)],"nextCursor":"c2"}
        """.data(using: .utf8)!
        let booksPage = try JSONFiles.decoder.decode(SearchPage.self, from: books)
        XCTAssertEqual(booksPage.books.map(\.id), ["b1"])
        XCTAssertEqual(booksPage.nextCursor, "c2")

        let posts = """
        {"type":"posts","items":[{"book":\(book),"post":{"id":"1","kind":"Body","text":"line","chapter":"One"}}],"nextCursor":null}
        """.data(using: .utf8)!
        let postsPage = try JSONFiles.decoder.decode(SearchPage.self, from: posts)
        XCTAssertEqual(postsPage.posts.map(\.post.id), ["1"])
        XCTAssertEqual(postsPage.posts.first?.book.ownerUsername, "ada")
    }

    func testDecodesProfileFeedAndReposts() throws {
        let profile = """
        {"username":"ada","joinedAt":1710000000000,"postCount":2,"repostCount":1,"mine":false}
        """.data(using: .utf8)!
        let record = try JSONFiles.decoder.decode(ProfileRecord.self, from: profile)
        XCTAssertEqual(record.postCount, 2)
        XCTAssertEqual(joinedLabel(record.joinedAt), "Joined Mar 2024")

        let posts = """
        {"items":[{"book":\(book),"post":{"id":"0","kind":"Heading","text":"Chapter","chapter":"One"},"comment":{"id":"k","username":"ada","text":"yes","createdAt":0,"mine":false}}],"nextCursor":null}
        """.data(using: .utf8)!
        let postPage = try JSONFiles.decoder.decode(ProfilePostsPage.self, from: posts)
        XCTAssertEqual(postPage.items.first?.comment.text, "yes")

        let reposts = """
        {"items":[{"book":\(book),"post":{"id":"1","kind":"Body","text":"line","chapter":"One"},"repostedAt":1710000000000}],"nextCursor":"n"}
        """.data(using: .utf8)!
        let repostPage = try JSONFiles.decoder.decode(ProfileRepostsPage.self, from: reposts)
        XCTAssertEqual(repostPage.items.first?.repostedAt, 1710000000000)
        XCTAssertEqual(repostPage.nextCursor, "n")

        let books = """
        {"items":[\(book)]}
        """.data(using: .utf8)!
        XCTAssertEqual(try JSONFiles.decoder.decode(ProfileBooksPage.self, from: books).items.map(\.title), ["Emma"])

        let state = """
        {"reposts":1,"reposted":true}
        """.data(using: .utf8)!
        XCTAssertEqual(try JSONFiles.decoder.decode(RepostState.self, from: state), RepostState(reposts: 1, reposted: true))

        let index = """
        {"counts":{"0":2},"mine":["0"],"reposts":{"0":1},"reposted":["0"]}
        """.data(using: .utf8)!
        let comments = try JSONFiles.decoder.decode(CommentIndex.self, from: index)
        XCTAssertEqual(comments.repostCount("0"), 1)
        XCTAssertTrue(comments.didRepost("0"))

        let legacy = """
        {"counts":{"0":2},"mine":["0"]}
        """.data(using: .utf8)!
        let old = try JSONFiles.decoder.decode(CommentIndex.self, from: legacy)
        XCTAssertEqual(old.reposts, [:])
        XCTAssertTrue(old.reposted.isEmpty)

        let feed = """
        {"items":[
          {"type":"comment","actor":{"username":"ada"},"book":\(book),"post":{"id":"0","kind":"Heading","text":"Chapter","chapter":"One"},"comment":{"id":"k","username":"ada","text":"yes","createdAt":0,"mine":false},"at":1710000000000},
          {"type":"repost","actor":{"username":"bea"},"book":\(book),"post":{"id":"1","kind":"Body","text":"line","chapter":"One"},"at":1710000000000}
        ],"nextCursor":null}
        """.data(using: .utf8)!
        let page = try JSONFiles.decoder.decode(HomeFeedPage.self, from: feed)
        XCTAssertEqual(page.items.map(\.actor), ["ada", "bea"])
        XCTAssertEqual(page.items[0].comment?.text, "yes")
        XCTAssertTrue(page.items[1].isRepost)
        XCTAssertNil(page.nextCursor)
    }
}

final class RepostTests: XCTestCase {
    func testOptimisticToggleRollsBackOnlyTheRequestThatFailed() {
        let start = CommentIndex(reposts: ["0": 2], reposted: [])
        let on = optimisticRepost(start.repostMark("0"))
        XCTAssertEqual(on, RepostMark(count: 3, on: true))
        var shown = start.applyingRepost("0", on)
        let off = optimisticRepost(shown.repostMark("0"))
        shown = shown.applyingRepost("0", off)
        let stale = endRepost(
            current: shown,
            postId: "0",
            sent: on,
            previous: start.repostMark("0"),
            result: .success(RepostState(reposts: 9, reposted: true))
        )
        XCTAssertEqual(stale.repostMark("0"), off)
        let failed = endRepost(
            current: shown,
            postId: "0",
            sent: off,
            previous: on,
            result: .failure(URLError(.notConnectedToInternet))
        )
        XCTAssertEqual(failed.repostMark("0"), on)
        let landed = endRepost(
            current: failed,
            postId: "0",
            sent: on,
            previous: start.repostMark("0"),
            result: .success(RepostState(reposts: 4, reposted: true))
        )
        XCTAssertEqual(landed.repostMark("0"), RepostMark(count: 4, on: true))
    }
}

@MainActor
final class HostedSearchModelTests: XCTestCase {
    private func until(_ remote: FakeRemoteSearch, _ condition: @escaping @MainActor () -> Bool) async -> Bool {
        if condition() { return true }
        let done = expectation(description: "search")
        remote.onCall = {
            if condition() {
                remote.onCall = nil
                done.fulfill()
            }
        }
        await fulfillment(of: [done], timeout: 5)
        return condition()
    }

    func testDebounceAndTabChangeCancelThePreviousRequest() async {
        let remote = FakeRemoteSearch()
        remote.token = "token-a"
        let model = SearchViewModel(library: StubLibrary(), remote: remote)
        model.setQuery("a")
        model.setQuery("ab")
        model.select(.books)
        guard await until(remote, { remote.calls.count == 1 }) else {
            return XCTFail("search did not start")
        }
        XCTAssertEqual(remote.calls.map(\.query), ["ab"])
        XCTAssertEqual(remote.calls.map(\.type), [.books])
        try? await Task.sleep(nanoseconds: 400_000_000)
        XCTAssertEqual(remote.calls.count, 1)
        remote.calls[0].continuation.resume(returning: SearchPage(type: .books, books: [
            BookRecord(id: "b", title: "ab", author: "A", handle: "a", importedAt: 0)
        ]))
        await model.searchTask?.value
        XCTAssertEqual(model.visibleBooks().map(\.id), ["b"])
        XCTAssertFalse(model.searching)
    }

    func testSessionSwitchDropsTheLatePage() async {
        let remote = FakeRemoteSearch()
        remote.token = "token-a"
        let model = SearchViewModel(library: StubLibrary(), remote: remote, debounceNanos: 0)
        model.setQuery("ada")
        guard await until(remote, { remote.calls.count == 1 }) else {
            return XCTFail("search did not start")
        }
        remote.token = "token-b"
        remote.calls[0].continuation.resume(returning: SearchPage(
            type: .accounts,
            accounts: [AccountRecord(username: "ada", mine: false)]
        ))
        await model.searchTask?.value
        XCTAssertTrue(model.visibleAccounts().isEmpty)
        XCTAssertFalse(model.showNoMatches())
    }

    func testSignOutClearsTheRowsAFinishedQueryHadShown() async {
        let remote = FakeRemoteSearch()
        remote.token = "token-a"
        let model = SearchViewModel(library: StubLibrary(), remote: remote, debounceNanos: 0)
        model.setQuery("ada")
        guard await until(remote, { remote.calls.count == 1 }) else {
            return XCTFail("search did not start")
        }
        remote.calls[0].continuation.resume(returning: SearchPage(
            type: .accounts,
            accounts: [AccountRecord(username: "ada", mine: false)]
        ))
        await model.searchTask?.value
        XCTAssertEqual(model.visibleAccounts().map(\.username), ["ada"])
        model.noteSession(nil)
        XCTAssertTrue(model.visibleAccounts().isEmpty)
        XCTAssertFalse(model.showNoMatches())
    }
}

@MainActor
final class ProfileTabTests: XCTestCase {
    private func until(_ loader: FakeProfileLoader, _ condition: @escaping @MainActor () -> Bool) async -> Bool {
        if condition() { return true }
        let done = expectation(description: "profile")
        loader.onCall = {
            if condition() {
                loader.onCall = nil
                done.fulfill()
            }
        }
        await fulfillment(of: [done], timeout: 5)
        return condition()
    }

    private func waitUntil(_ condition: @escaping @MainActor () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(3)
        while !condition() {
            if Date() > deadline { return false }
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        return true
    }

    func testTabsLoadAfterTheProfileAndAMissingAccountIs404() async {
        let loader = FakeProfileLoader()
        let model = ProfileViewModel(loader: loader, library: StubLibrary(), handle: "ada")
        model.refresh()
        guard await until(loader, { loader.profiles.count == 1 }) else {
            return XCTFail("profile did not start")
        }
        XCTAssertTrue(model.headerLoading)
        loader.profiles[0].resume(returning: ProfileRecord(
            username: "ada", joinedAt: 1710000000000, postCount: 2, repostCount: 1, mine: false
        ))
        guard await until(loader, { loader.postPages.count == 1 }) else {
            return XCTFail("posts did not start")
        }
        XCTAssertEqual(model.postCount, 2)
        XCTAssertTrue(model.postsLoading)
        XCTAssertFalse(model.postsLoaded)
        let item = ProfilePostItem(
            book: BookRecord(id: "b", title: "Emma", author: "Austen", handle: "austen", importedAt: 0),
            post: FeedPost(id: "0", kind: .body, text: "line", chapter: "One"),
            comment: PostComment(id: "k", username: "ada", text: "yes", createdAt: 0, mine: false)
        )
        loader.postPages[0].resume(returning: ProfilePostsPage(items: [item], nextCursor: nil))
        guard await waitUntil({ model.postsLoaded }) else {
            return XCTFail("posts did not load")
        }
        XCTAssertEqual(model.posts.map(\.comment.text), ["yes"])
        XCTAssertEqual(loader.feedLooks, 0)

        model.select(.reposts)
        guard await until(loader, { loader.repostPages.count == 1 }) else {
            return XCTFail("reposts did not start")
        }
        XCTAssertTrue(model.repostsLoading)
        loader.repostPages[0].resume(returning: ProfileRepostsPage(items: [], nextCursor: nil))
        guard await waitUntil({ model.repostsLoaded }) else {
            return XCTFail("reposts did not load")
        }
        XCTAssertTrue(model.reposts.isEmpty)

        model.select(.books)
        guard await until(loader, { loader.bookPages.count == 1 }) else {
            return XCTFail("books did not start")
        }
        loader.bookPages[0].resume(returning: ProfileBooksPage(items: []))
        guard await waitUntil({ model.booksLoaded }) else {
            return XCTFail("books did not load")
        }
        XCTAssertTrue(model.books.isEmpty)
    }

    func testUnknownAccountAndALatePageAfterSignOut() async {
        let loader = FakeProfileLoader()
        let missing = ProfileViewModel(loader: loader, library: StubLibrary(), handle: "gone")
        missing.refresh()
        guard await until(loader, { loader.profiles.count == 1 }) else {
            return XCTFail("profile did not start")
        }
        loader.profiles[0].resume(throwing: HostedException(message: "nope", code: 404))
        guard await waitUntil({ missing.missing }) else {
            return XCTFail("404 did not settle")
        }
        XCTAssertFalse(missing.headerLoading)

        let late = FakeProfileLoader()
        let model = ProfileViewModel(loader: late, library: StubLibrary(), handle: "ada")
        model.refresh()
        guard await until(late, { late.profiles.count == 1 }) else {
            return XCTFail("profile did not start")
        }
        late.profiles[0].resume(returning: ProfileRecord(
            username: "ada", joinedAt: 1, postCount: 1, repostCount: 0, mine: true
        ))
        guard await until(late, { late.postPages.count == 1 }) else {
            return XCTFail("posts did not start")
        }
        late.token = nil
        late.postPages[0].resume(returning: ProfilePostsPage(items: [
            ProfilePostItem(
                book: BookRecord(id: "b", title: "Emma", author: "A", handle: "a", importedAt: 0),
                post: FeedPost(id: "0", kind: .body, text: "secret", chapter: "One"),
                comment: PostComment(id: "k", username: "ada", text: "secret", createdAt: 0)
            )
        ], nextCursor: nil))
        await Task.yield()
        try? await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertTrue(model.posts.isEmpty)
        XCTAssertFalse(model.postsLoaded)
    }
}

final class FeedLoadTests: XCTestCase {
    func testEmptyCopyWaitsForASuccessfulFetch() {
        XCTAssertFalse(FeedLoad.loading.showEmpty)
        XCTAssertFalse(FeedLoad.signedOut.showEmpty)
        let row = feedItem("cached")
        let cached = reduceFeedLoad(.loading, .cached([row], nil))
        XCTAssertFalse(cached.showEmpty)
        XCTAssertTrue(cached.isRefreshing)
        XCTAssertEqual(cached.items.map(\.post.text), ["cached"])
        let empty = reduceFeedLoad(.loading, .refreshSucceeded([], nil))
        XCTAssertTrue(empty.showEmpty)
        let failed = reduceFeedLoad(cached, .refreshFailed("offline"))
        XCTAssertFalse(failed.showEmpty)
        XCTAssertEqual(failed.items.map(\.post.text), ["cached"])
        XCTAssertEqual(failed.failureMessage, "offline")
        XCTAssertEqual(reduceFeedLoad(failed, .signedOut), .signedOut)
        let first = reduceFeedLoad(.loading, .refreshFailed("offline"))
        XCTAssertTrue(first.isFailedEmpty)
        XCTAssertFalse(first.showEmpty)
    }
}

@MainActor
final class HomeFeedStoreTests: XCTestCase {
    private var root: URL!
    private var suite: String!
    private var defaults: UserDefaults!
    private var sessions: HostedSessionStore!
    private var gate: FeedGate!
    private var store: HomeFeedStore!

    override func setUp() async throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        suite = "mewo-feed-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)
        sessions = HostedSessionStore(defaults: defaults, persistToken: false)
        sessions.setBaseUrl("https://mewo.example")
        gate = FeedGate()
        store = HomeFeedStore(sessions: sessions, client: gate.client, files: root, refreshesOnInit: false)
    }

    override func tearDown() async throws {
        gate.failAll()
        sessions.signOut()
        store = nil
        defaults.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: root)
    }

    func testCachedPageShowsImmediatelyAndASessionSwitchDropsTheLatePage() async throws {
        let ada = HomeFeedPage(items: [feedItem("ada secret")], nextCursor: nil)
        try JSONFiles.write(ada, to: feedFile(user: "ada"))
        sessions.setSignedIn(username: "ada", token: "token-a")
        XCTAssertEqual(store.loadState.items.map(\.post.text), ["ada secret"])
        XCTAssertTrue(store.loadState.isRefreshing)
        XCTAssertFalse(store.loadState.showEmpty)
        try await gate.waitForCalls(1)

        try JSONFiles.write(HomeFeedPage(items: [feedItem("bea")], nextCursor: nil), to: feedFile(user: "bea"))
        sessions.setSignedIn(username: "bea", token: "token-b")
        XCTAssertEqual(store.loadState.items.map(\.post.text), ["bea"])
        XCTAssertFalse(FileManager.default.fileExists(atPath: feedFile(user: "ada").path))
        gate.respond("token-a", page: HomeFeedPage(items: [feedItem("ada late")], nextCursor: nil))
        try await Task.sleep(nanoseconds: 200_000_000)
        XCTAssertFalse(store.loadState.items.contains { $0.post.text == "ada late" })
        XCTAssertFalse(store.loadState.items.contains { $0.post.text == "ada secret" })

        try await gate.waitForCalls(2)
        gate.respond("token-b", page: HomeFeedPage(items: [], nextCursor: nil))
        try await waitUntil { store.loadState.showEmpty && !store.loadState.isRefreshing }
        XCTAssertTrue(store.loadState.items.isEmpty)

        sessions.signOut()
        XCTAssertEqual(store.loadState, .signedOut)
        XCTAssertFalse(store.loadState.showEmpty)
        XCTAssertFalse(FileManager.default.fileExists(atPath: feedFile(user: "bea").path))
    }

    private func feedFile(user: String) -> URL {
        let key = shelfCacheKey(serverUrl: "https://mewo.example", username: user)
        return root.appendingPathComponent("hosted-feeds").appendingPathComponent(key + ".json")
    }

    private func waitUntil(_ condition: @MainActor () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(3)
        while !condition() {
            if Date() > deadline {
                XCTFail("feed did not settle: \(store.loadState)")
                return
            }
            try await Task.sleep(nanoseconds: 20_000_000)
        }
    }
}

private func feedItem(_ text: String) -> HomeFeedItem {
    HomeFeedItem(
        type: "repost",
        actor: "ada",
        book: BookRecord(id: "b1", title: "Emma", author: "Austen", handle: "austen", importedAt: 0),
        post: FeedPost(id: "1", kind: .body, text: text, chapter: "One"),
        comment: nil,
        at: 1
    )
}

@MainActor
private final class FakeRemoteSearch: RemoteSearch {
    var token: String?
    var calls: [Call] = []
    var onCall: (() -> Void)?

    struct Call {
        var query: String
        var type: SearchTab
        var continuation: CheckedContinuation<SearchPage, Error>
    }

    func search(query: String, type: SearchTab, cursor: String?) async throws -> SearchPage {
        try await withCheckedThrowingContinuation { cont in
            calls.append(Call(query: query, type: type, continuation: cont))
            onCall?()
        }
    }
}

@MainActor
private final class FakeProfileLoader: ProfileLoading {
    var token: String? = "token-a"
    var profiles: [CheckedContinuation<ProfileRecord, Error>] = []
    var postPages: [CheckedContinuation<ProfilePostsPage, Error>] = []
    var repostPages: [CheckedContinuation<ProfileRepostsPage, Error>] = []
    var bookPages: [CheckedContinuation<ProfileBooksPage, Error>] = []
    var feedLooks = 0
    var onCall: (() -> Void)?

    func loadProfile(username: String) async throws -> ProfileRecord {
        try await withCheckedThrowingContinuation { cont in
            profiles.append(cont)
            onCall?()
        }
    }

    func loadPosts(username: String, cursor: String?) async throws -> ProfilePostsPage {
        try await withCheckedThrowingContinuation { cont in
            postPages.append(cont)
            onCall?()
        }
    }

    func loadReposts(username: String, cursor: String?) async throws -> ProfileRepostsPage {
        try await withCheckedThrowingContinuation { cont in
            repostPages.append(cont)
            onCall?()
        }
    }

    func loadBooks(username: String) async throws -> ProfileBooksPage {
        try await withCheckedThrowingContinuation { cont in
            bookPages.append(cont)
            onCall?()
        }
    }
}

@MainActor
private final class StubLibrary: LibraryStore {
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

private final class FeedGate: @unchecked Sendable {
    let client = HostedClient()
    private let lock = NSLock()
    private var calls = 0
    private var gates: [String: CheckedContinuation<HomeFeedPage, Error>] = [:]
    private var waiters: [Waiter] = []

    init() {
        client.hooks.feed = { [weak self] session, _ in
            guard let self else { throw CancellationError() }
            return try await self.hold(session.token ?? "")
        }
    }

    func waitForCalls(_ count: Int) async throws {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            lock.lock()
            if calls >= count {
                lock.unlock()
                cont.resume()
                return
            }
            waiters.append(Waiter(count: count, cont: cont))
            lock.unlock()
        }
    }

    func respond(_ token: String, page: HomeFeedPage) {
        lock.lock()
        let cont = gates.removeValue(forKey: token)
        lock.unlock()
        cont?.resume(returning: page)
    }

    func failAll() {
        lock.lock()
        let pending = gates
        gates.removeAll()
        let starts = waiters
        waiters.removeAll()
        lock.unlock()
        for cont in pending.values { cont.resume(throwing: CancellationError()) }
        for start in starts { start.cont.resume(throwing: CancellationError()) }
    }

    private func hold(_ token: String) async throws -> HomeFeedPage {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<HomeFeedPage, Error>) in
            lock.lock()
            calls += 1
            gates[token] = cont
            let ready = waiters.filter { calls >= $0.count }
            waiters.removeAll { calls >= $0.count }
            lock.unlock()
            for start in ready { start.cont.resume() }
        }
    }

    private struct Waiter {
        var count: Int
        var cont: CheckedContinuation<Void, Error>
    }
}
