import XCTest
@testable import Mewo

final class LibraryLoadTests: XCTestCase {
    func testEmptyCopyWaitsForASuccessfulFetch() {
        XCTAssertFalse(LibraryLoad.loading.showEmpty)
        XCTAssertTrue(LibraryLoad.loading.books.isEmpty)

        let none = reduceLibraryLoad(.loading, .refreshSucceeded([]))
        XCTAssertTrue(none.showEmpty)
        XCTAssertFalse(none.isRefreshing)

        let cached = reduceLibraryLoad(.loading, .cached([book("a")]))
        XCTAssertFalse(cached.showEmpty)
        XCTAssertTrue(cached.isRefreshing)
        let refreshing = reduceLibraryLoad(cached, .refreshStarted)
        XCTAssertTrue(refreshing.isRefreshing)
        XCTAssertEqual(refreshing.books.map(\.id), ["a"])
    }

    func testFailedRefreshKeepsTheSavedShelf() {
        let saved = book("a", shared: true, mine: false, owner: "ada", progress: 4)
        let loaded = LibraryLoad.loaded(books: [saved], refreshing: true)
        let failed = reduceLibraryLoad(loaded, .refreshFailed("offline"))
        guard case .failed(let message, let books) = failed else {
            return XCTFail("expected a failure, got \(failed)")
        }
        XCTAssertEqual(message, "offline")
        XCTAssertEqual(books?.first?.progressIndex, 4)
        XCTAssertEqual(books?.first?.ownerUsername, "ada")
        XCTAssertEqual(books?.first?.shared, true)
        XCTAssertEqual(books?.first?.mine, false)
        XCTAssertFalse(failed.showEmpty)

        let first = reduceLibraryLoad(.loading, .refreshFailed("offline"))
        guard case .failed(_, nil) = first else {
            return XCTFail("a first failure has no shelf")
        }
        XCTAssertFalse(first.showEmpty)
        XCTAssertEqual(reduceLibraryLoad(first, .refreshStarted), .loading)
    }

    func testDismissingACachedFailureReturnsToTheShelf() {
        let failed = LibraryLoad.failed(message: "offline", books: [book("a")])
        let dismissed = reduceLibraryLoad(failed, .dismissed)
        XCTAssertEqual(dismissed, .loaded(books: [book("a")], refreshing: false))
    }

    func testSavedTextOpensOnlyWhileTheShelfIsCatchingUp() {
        let id = "33333333-3333-3333-3333-333333333333"
        XCTAssertTrue(savedTextAllowed(.loading, bookId: id))
        XCTAssertTrue(savedTextAllowed(.loaded(books: [book(id)], refreshing: true), bookId: id))
        XCTAssertTrue(savedTextAllowed(.failed(message: "offline", books: nil), bookId: id))
        XCTAssertFalse(savedTextAllowed(.loaded(books: [book("other")], refreshing: false), bookId: id))
        XCTAssertTrue(savedTextAllowed(.loaded(books: [book(id)], refreshing: false), bookId: id))
    }

    func testShelfFilesArePerServerAndUsername() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let cache = ShelfCache(directory: dir)
        let ada = [book("a", shared: false, mine: true, owner: "ada", progress: 8)]
        let matt = [book("b", shared: true, mine: false, owner: "matt", progress: 1)]
        try cache.write(serverUrl: "https://mewo.example", username: "ada", books: ada)
        try cache.write(serverUrl: "https://mewo.example", username: "matt", books: matt)
        try cache.write(serverUrl: "http://other.example", username: "ada", books: [book("c")])
        XCTAssertNotEqual(
            shelfCacheKey(serverUrl: "https://mewo.example", username: "ada"),
            shelfCacheKey(serverUrl: "https://mewo.example", username: "matt")
        )
        XCTAssertNotEqual(
            shelfCacheKey(serverUrl: "https://mewo.example", username: "ada"),
            shelfCacheKey(serverUrl: "http://other.example", username: "ada")
        )
        XCTAssertEqual(
            shelfCacheKey(serverUrl: "https://mewo.example/", username: "ada"),
            shelfCacheKey(serverUrl: "https://mewo.example", username: "ada")
        )
        XCTAssertEqual(cache.read(serverUrl: "https://mewo.example", username: "ada")?.first?.id, "a")
        XCTAssertEqual(cache.read(serverUrl: "https://mewo.example", username: "ada")?.first?.progressIndex, 8)
        XCTAssertEqual(cache.read(serverUrl: "https://mewo.example", username: "matt")?.first?.ownerUsername, "matt")
        XCTAssertEqual(cache.read(serverUrl: "https://mewo.example", username: "matt")?.first?.shared, true)
        XCTAssertEqual(cache.read(serverUrl: "http://other.example", username: "ada")?.first?.id, "c")
        cache.clear(serverUrl: "https://mewo.example", username: "ada")
        XCTAssertNil(cache.read(serverUrl: "https://mewo.example", username: "ada"))
        XCTAssertEqual(cache.read(serverUrl: "https://mewo.example", username: "matt")?.first?.id, "b")
    }

    private func book(
        _ id: String,
        shared: Bool = false,
        mine: Bool = true,
        owner: String = "ada",
        progress: Int = 0
    ) -> BookRecord {
        BookRecord(
            id: id,
            title: id,
            author: "Austen",
            handle: "austen",
            importedAt: 1,
            postCount: 12,
            progressIndex: progress,
            mine: mine,
            shared: shared,
            ownerUsername: owner
        )
    }
}
