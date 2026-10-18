import XCTest
@testable import Mewo

final class SearchScanTests: XCTestCase {
    private func book(_ id: String, title: String = "Book") -> BookRecord {
        BookRecord(id: id, title: title, author: "Author", handle: "author", importedAt: 0)
    }

    private func source(_ book: BookRecord, posts: [String], version: Int = 1) throws -> SearchSource {
        let file = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathComponent("feed.json")
        let rows = posts.enumerated().map { FeedPost(id: "\($0.offset)", kind: .body, text: $0.element, chapter: "One") }
        try JSONFiles.write(FeedCache(version: version, posts: rows), to: file)
        return SearchSource(book: book, feedFile: file, cacheVersion: 1)
    }

    func testMatchesBooksAndCachedPosts() async throws {
        let a = try source(book("a", title: "Moby Dick"), posts: ["Call me Ishmael", "whale"])
        let b = try source(book("b"), posts: ["another whale"])
        let result = try await scanSearch(query: " WHALE ", sources: [a, b])
        XCTAssertEqual(result.books, [])
        XCTAssertEqual(result.posts.map(\.id), ["a:1", "b:0"])
        let byTitle = try await scanSearch(query: "moby", sources: [a, b])
        XCTAssertEqual(byTitle.books.map(\.id), ["a"])
    }

    func testSkipsStaleCacheVersion() async throws {
        let old = try source(book("a"), posts: ["whale"], version: 0)
        let result = try await scanSearch(query: "whale", sources: [old])
        XCTAssertEqual(result.posts, [])
    }

    func testStopsWhenCancelled() async throws {
        let a = try source(book("a"), posts: ["whale"])
        let task = Task { () async throws -> SearchResult in
            withUnsafeCurrentTask { $0?.cancel() }
            return try await scanSearch(query: "whale", sources: [a])
        }
        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch is CancellationError {}
    }
}

@MainActor
final class SearchViewModelTests: XCTestCase {
    private func hits(_ title: String) -> SearchResult {
        let book = BookRecord(id: title, title: title, author: "Author", handle: "author", importedAt: 0)
        return SearchResult(books: [book], posts: [PostHit(book: book, post: FeedPost(id: "p", kind: .body, text: title, chapter: ""))])
    }

    /// Waits until the fake store reports `condition`. The store calls back on
    /// every search, likes, or comment index call, so there is no polling budget.
    private func until(_ store: FakeSearchStore, _ condition: @escaping @MainActor () -> Bool) async -> Bool {
        if condition() { return true }
        let done = expectation(description: "store reached state")
        store.onCall = {
            if condition() {
                store.onCall = nil
                done.fulfill()
            }
        }
        await fulfillment(of: [done], timeout: 5)
        return condition()
    }

    /// Types `query`, answers its scan with `result`, and waits for the scan.
    private func search(_ model: SearchViewModel, _ store: FakeSearchStore, _ query: String, _ result: SearchResult) async -> Bool {
        let count = store.searches.count
        model.setQuery(query)
        guard await until(store, { store.searches.count == count + 1 }) else { return false }
        store.searches[count].resume(returning: result)
        await model.searchTask?.value
        return true
    }

    /// Reopens the screen, answers its scan with `result`, and waits for the scan.
    private func refresh(_ model: SearchViewModel, _ store: FakeSearchStore, _ result: SearchResult) async -> Bool {
        let count = store.searches.count
        model.refresh()
        guard await until(store, { store.searches.count == count + 1 }) else { return false }
        store.searches[count].resume(returning: result)
        await model.searchTask?.value
        return true
    }

    func testOnlyNewestSearchPublishesEvenForRepeatedQuery() async {
        let store = FakeSearchStore()
        let model = SearchViewModel(library: store)
        model.setQuery("a")
        let first = model.searchTask
        model.setQuery("ab")
        let middle = model.searchTask
        model.setQuery("a")
        XCTAssertEqual(model.query, "a")
        let last = model.searchTask
        guard await until(store, { store.searches.count == 3 }) else { return }

        store.searches[2].resume(returning: hits("new"))
        await last?.value
        store.searches[0].resume(returning: hits("old"))
        store.searches[1].resume(returning: hits("middle"))
        await first?.value
        await middle?.value
        await model.social(for: "new")?.pending?.value
        XCTAssertEqual(model.result, hits("new"))
        XCTAssertEqual(store.likeIds, ["new"])
        XCTAssertNil(model.social(for: "old"))
    }

    func testHitsPublishBeforeLikesLoadAndStayCachedUntilRefresh() async {
        let store = FakeSearchStore()
        store.holdLikes = true
        let model = SearchViewModel(library: store)
        guard await search(model, store, "a", hits("a")) else { return }
        guard await until(store, { store.likeCalls.count == 1 }) else { return }
        XCTAssertEqual(model.result, hits("a"))
        XCTAssertNil(model.social(for: "a")?.likes)

        store.likeCalls[0].resume(returning: ["p"])
        await model.social(for: "a")?.pending?.value
        XCTAssertEqual(model.social(for: "a")?.likes, ["p"])

        // The same book does not load again while it stays cached.
        guard await search(model, store, "ab", hits("a")) else { return }
        XCTAssertNil(model.social(for: "a")?.pending)
        XCTAssertEqual(store.likeIds, ["a"])

        // Reappearing reloads it, keeping the old likes on screen meanwhile.
        guard await refresh(model, store, hits("a")) else { return }
        guard await until(store, { store.likeCalls.count == 2 }) else { return }
        XCTAssertEqual(model.social(for: "a")?.likes, ["p"])
        store.likeCalls[1].resume(returning: [])
        await model.social(for: "a")?.pending?.value
        XCTAssertEqual(model.social(for: "a")?.likes, [])
    }

    /// Typing while a book's likes load joins that load instead of starting
    /// another. The fake ignores cancellation, like a slow request.
    func testTypingReusesPendingBookLoad() async {
        let store = FakeSearchStore()
        store.holdLikes = true
        let model = SearchViewModel(library: store)
        guard await search(model, store, "a", hits("a")) else { return }
        guard await until(store, { store.likeCalls.count == 1 }) else { return }
        let load = model.social(for: "a")?.pending

        guard await search(model, store, "ab", hits("a")) else { return }
        XCTAssertEqual(model.social(for: "a")?.pending, load)
        model.setQuery("")
        XCTAssertEqual(model.social(for: "a")?.pending, load)

        store.likeCalls[0].resume(returning: ["p"])
        await load?.value
        XCTAssertEqual(store.likeIds, ["a"])
        XCTAssertEqual(store.commentReads, 1)
        XCTAssertEqual(model.social(for: "a")?.likes, ["p"])
    }

    /// A failed field keeps its last value and loads again on the next hit;
    /// a field that loaded does not.
    func testFailedFieldRetriesAndKeepsLastValue() async {
        let store = FakeSearchStore()
        let model = SearchViewModel(library: store)
        store.commentFailures = 1
        guard await search(model, store, "a", hits("a")) else { return }
        await model.social(for: "a")?.pending?.value
        XCTAssertNil(model.social(for: "a")?.comments)

        store.index = CommentIndex(counts: ["p": 4])
        guard await search(model, store, "ab", hits("a")) else { return }
        await model.social(for: "a")?.pending?.value
        XCTAssertEqual(store.commentReads, 2)
        XCTAssertEqual(store.likeIds, ["a"])
        XCTAssertEqual(model.social(for: "a")?.comments?.count("p"), 4)

        // A failed reload keeps the counts already shown, then retries.
        store.commentFailures = 1
        store.index = CommentIndex(counts: ["p": 5])
        guard await refresh(model, store, hits("a")) else { return }
        await model.social(for: "a")?.pending?.value
        XCTAssertEqual(model.social(for: "a")?.comments?.count("p"), 4)
        guard await search(model, store, "a", hits("a")) else { return }
        await model.social(for: "a")?.pending?.value
        XCTAssertEqual(store.commentReads, 4)
        XCTAssertEqual(store.likeIds, ["a", "a"])
        XCTAssertEqual(model.social(for: "a")?.comments?.count("p"), 5)
    }

    /// A like or comment made while a load is in flight wins over that load's
    /// older answer, and the edited thread keeps the other posts' counts.
    func testUserEditsWinOverInFlightSocialLoad() async {
        let store = FakeSearchStore()
        store.holdLikes = true
        let model = SearchViewModel(library: store)
        guard await search(model, store, "a", hits("a")) else { return }
        guard await until(store, { store.likeCalls.count == 1 }) else { return }

        store.toggled = ["p"]
        await model.toggleLike(bookId: "a", postId: "p").value
        XCTAssertEqual(model.social(for: "a")?.likes, ["p"])
        store.index = CommentIndex(counts: ["other": 3])
        let reply = PostComment(id: "c", username: "me", text: "hi", createdAt: 0, mine: true)
        model.onCommentsChanged(bookId: "a", postId: "p", comments: [reply])
        XCTAssertEqual(model.social(for: "a")?.comments?.count("p"), 1)

        store.likeCalls[0].resume(returning: [])
        await model.social(for: "a")?.pending?.value
        let social = model.social(for: "a")
        XCTAssertEqual(social?.likes, ["p"])
        XCTAssertEqual(social?.comments?.count("p"), 1)
        XCTAssertEqual(social?.comments?.commented("p"), true)
        XCTAssertEqual(social?.comments?.count("other"), 3)

        // An edit after the load lands merges into the loaded counts.
        model.onCommentsChanged(bookId: "a", postId: "q", comments: [reply, reply])
        XCTAssertEqual(social?.comments?.count("q"), 2)
        XCTAssertEqual(social?.comments?.count("other"), 3)

        // The toggle counts as fresh likes, so a new query loads nothing.
        guard await search(model, store, "ab", hits("a")) else { return }
        XCTAssertNil(model.social(for: "a")?.pending)
        XCTAssertEqual(store.likeIds, ["a"])
    }

    /// A load in flight when the screen reappears may predate changes made in
    /// the reader, so its answer is dropped and a new load replaces it.
    func testRefreshDropsInFlightLoad() async {
        let store = FakeSearchStore()
        store.holdLikes = true
        let model = SearchViewModel(library: store)
        guard await search(model, store, "a", hits("a")) else { return }
        guard await until(store, { store.likeCalls.count == 1 }) else { return }
        let old = model.social(for: "a")?.pending

        guard await refresh(model, store, hits("a")) else { return }
        guard await until(store, { store.likeCalls.count == 2 }) else { return }
        XCTAssertNotEqual(model.social(for: "a")?.pending, old)
        store.likeCalls[0].resume(returning: ["stale"])
        await old?.value
        XCTAssertNil(model.social(for: "a")?.likes)

        store.likeCalls[1].resume(returning: ["fresh"])
        await model.social(for: "a")?.pending?.value
        XCTAssertEqual(model.social(for: "a")?.likes, ["fresh"])
    }
}

/// Library whose searches wait until the test resumes them. Likes answer at
/// once unless `holdLikes` is set. Comment index reads fail while
/// `commentFailures` is above zero. Nothing honors cancellation.
@MainActor
private final class FakeSearchStore: LibraryStore {
    var books: [BookRecord] = []
    var loadState: LibraryLoad = .loaded(books: [], refreshing: false)
    var holdLikes = false
    var searches: [CheckedContinuation<SearchResult, Never>] = []
    var likeCalls: [CheckedContinuation<Set<String>, Never>] = []
    var likeIds: [String] = []
    var toggled: Set<String> = []
    var index = CommentIndex()
    var commentFailures = 0
    var commentReads = 0
    /// Runs after each search, likes, or comment index call is recorded.
    var onCall: (() -> Void)?

    func search(query: String) async throws -> SearchResult {
        await withCheckedContinuation {
            searches.append($0)
            onCall?()
        }
    }
    func likes(id: String) async throws -> Set<String> {
        likeIds.append(id)
        if !holdLikes {
            onCall?()
            return []
        }
        return await withCheckedContinuation {
            likeCalls.append($0)
            onCall?()
        }
    }
    func commentIndex(id: String) async throws -> CommentIndex {
        commentReads += 1
        onCall?()
        if commentFailures > 0 {
            commentFailures -= 1
            throw URLError(.notConnectedToInternet)
        }
        return index
    }

    func load() async throws {}
    func dismissLoadError() {}
    func importFromFile(_ file: URL) async throws -> BookRecord { throw CancellationError() }
    func importSample() async throws -> BookRecord { throw CancellationError() }
    func delete(id: String) async throws {}
    func book(id: String) -> BookRecord? { nil }
    func feed(id: String) async throws -> [FeedPost] { [] }
    func saveProgress(id: String, index: Int) async throws {}
    func toggleLike(id: String, postId: String) async throws -> Set<String> {
        toggled
    }
    func comments(id: String, postId: String) async throws -> [PostComment] { [] }
    func addComment(id: String, postId: String, text: String) async throws -> [PostComment] { [] }
    func deleteComment(id: String, postId: String, commentId: String) async throws -> [PostComment] { [] }
    func likedPosts() async throws -> [PostHit] { [] }
    func profileReplies(handle: String) async throws -> [ProfileReply] { [] }
    func coverFile(id: String) -> URL? { nil }
}
