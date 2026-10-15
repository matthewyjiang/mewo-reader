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
    /// every search or likes call, so there is no polling budget.
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
        XCTAssertEqual(model.result, hits("new"))
        XCTAssertEqual(store.likeIds, ["new"])
    }

    func testHitsPublishBeforeLikesLoad() async {
        let store = FakeSearchStore()
        store.holdLikes = true
        let model = SearchViewModel(library: store)
        model.setQuery("a")
        guard await until(store, { store.searches.count == 1 }) else { return }
        store.searches[0].resume(returning: hits("a"))
        guard await until(store, { store.likeCalls.count == 1 }) else { return }
        XCTAssertEqual(model.result, hits("a"))
        XCTAssertNil(model.likes["a"])

        store.likeCalls[0].resume(returning: ["p"])
        await model.searchTask?.value
        XCTAssertEqual(model.likes["a"], ["p"])

        // The same book does not load again until the screen reappears.
        model.setQuery("ab")
        guard await until(store, { store.searches.count == 2 }) else { return }
        store.searches[1].resume(returning: hits("a"))
        await model.searchTask?.value
        XCTAssertEqual(store.likeCalls.count, 1)

        model.refresh()
        guard await until(store, { store.searches.count == 3 }) else { return }
        store.searches[2].resume(returning: hits("a"))
        guard await until(store, { store.likeCalls.count == 2 }) else { return }
        store.likeCalls[1].resume(returning: [])
        await model.searchTask?.value
        XCTAssertEqual(model.likes["a"], [])
    }

    /// The fake ignores cancellation, like a slow request that still answers.
    func testStaleLikesDoNotPublish() async {
        let store = FakeSearchStore()
        store.holdLikes = true
        let model = SearchViewModel(library: store)
        model.setQuery("a")
        guard await until(store, { store.searches.count == 1 }) else { return }
        let old = model.searchTask
        store.searches[0].resume(returning: hits("a"))
        guard await until(store, { store.likeCalls.count == 1 }) else { return }

        // A new query lands while likes for "a" are in flight.
        model.setQuery("b")
        store.likeCalls[0].resume(returning: ["stale"])
        await old?.value
        XCTAssertNil(model.likes["a"])

        // The new search finds the same book, so it loads again.
        guard await until(store, { store.searches.count == 2 }) else { return }
        let second = model.searchTask
        store.searches[1].resume(returning: hits("a"))
        guard await until(store, { store.likeCalls.count == 2 }) else { return }

        // A refresh lands while that load is in flight.
        model.refresh()
        store.likeCalls[1].resume(returning: ["stale"])
        await second?.value
        XCTAssertNil(model.likes["a"])

        guard await until(store, { store.searches.count == 3 }) else { return }
        store.searches[2].resume(returning: hits("a"))
        guard await until(store, { store.likeCalls.count == 3 }) else { return }
        store.likeCalls[2].resume(returning: ["fresh"])
        await model.searchTask?.value
        XCTAssertEqual(model.likes["a"], ["fresh"])
    }

    /// A like or comment made while the first load is in flight wins over
    /// that load's older answer.
    func testUserEditsWinOverInFlightSocialLoad() async {
        let store = FakeSearchStore()
        store.holdLikes = true
        let model = SearchViewModel(library: store)
        model.setQuery("a")
        guard await until(store, { store.searches.count == 1 }) else { return }
        store.searches[0].resume(returning: hits("a"))
        guard await until(store, { store.likeCalls.count == 1 }) else { return }

        store.toggled = ["p"]
        await model.toggleLike(bookId: "a", postId: "p").value
        XCTAssertEqual(model.likes["a"], ["p"])
        store.index = CommentIndex(counts: ["other": 3])
        let reply = PostComment(id: "c", username: "me", text: "hi", createdAt: 0, mine: true)
        model.onCommentsChanged(bookId: "a", postId: "p", comments: [reply])

        store.likeCalls[0].resume(returning: [])
        await model.searchTask?.value
        XCTAssertEqual(model.likes["a"], ["p"])
        XCTAssertEqual(model.comments["a"]?.count("p"), 1)
        XCTAssertEqual(model.comments["a"]?.commented("p"), true)
        XCTAssertEqual(model.comments["a"]?.count("other"), 3)
    }
}

/// Library whose searches wait until the test resumes them. Likes answer at
/// once unless `holdLikes` is set. Neither honors cancellation.
@MainActor
private final class FakeSearchStore: LibraryStore {
    var books: [BookRecord] = []
    var holdLikes = false
    var searches: [CheckedContinuation<SearchResult, Never>] = []
    var likeCalls: [CheckedContinuation<Set<String>, Never>] = []
    var likeIds: [String] = []
    var toggled: Set<String> = []
    var index = CommentIndex()
    /// Runs after each search or likes call is recorded.
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
    func commentIndex(id: String) async throws -> CommentIndex { index }

    func load() async throws {}
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
