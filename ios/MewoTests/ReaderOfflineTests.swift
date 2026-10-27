import XCTest
@testable import Mewo

@MainActor
final class ReaderOfflineTests: XCTestCase {
    func testCachedBookOpensWhenSocialReadsFail() async {
        let store = FakeReaderStore()
        store.likesError = offlineError()
        store.indexError = offlineError()
        let vm = ReaderViewModel(library: store, bookId: readerBook)

        await vm.load()

        XCTAssertNil(vm.error)
        XCTAssertFalse(vm.loading)
        XCTAssertEqual(vm.posts.map(\.id), ["p1"])
        XCTAssertEqual(vm.book?.title, "Pride")
        XCTAssertTrue(vm.likes.isEmpty)
        XCTAssertEqual(vm.comments.count("p1"), 0)
    }

    func testAFeedFailureStillFailsToOpen() async {
        let store = FakeReaderStore()
        store.feedError = NSError(domain: "mewo", code: 1, userInfo: [NSLocalizedDescriptionKey: "no posts"])
        let vm = ReaderViewModel(library: store, bookId: readerBook)

        await vm.load()

        XCTAssertEqual(vm.error, "no posts")
        XCTAssertTrue(vm.posts.isEmpty)
        XCTAssertFalse(vm.loading)
    }

    func testOfflineLikeDoesNotCrashAndReverts() async throws {
        let store = FakeReaderStore()
        let vm = ReaderViewModel(library: store, bookId: readerBook)
        await vm.load()
        XCTAssertEqual(vm.likes, ["p1"])

        store.likeError = offlineError()
        store.armLikes()
        vm.toggleLike("p1")
        try await waitUntil("like did not start") { store.likesStarted == 1 }
        XCTAssertFalse(vm.likes.contains("p1"))

        store.releaseLikes()
        try await waitUntil("like did not revert") { vm.likes == ["p1"] }
        XCTAssertNil(vm.error)
        XCTAssertEqual(vm.posts.map(\.id), ["p1"])
    }

    func testTwoFailedLikesRevertOnlyTheirOwnPost() async throws {
        let store = FakeReaderStore()
        let vm = ReaderViewModel(library: store, bookId: readerBook)
        await vm.load()

        store.likeError = offlineError()
        store.armLikes()
        vm.toggleLike("p1")
        try await waitUntil("first like did not start") { store.likesStarted == 1 }
        vm.toggleLike("p2")
        try await waitUntil("second like did not start") { store.likesStarted == 2 }
        XCTAssertEqual(vm.likes, ["p2"])

        store.releaseLikes()
        try await waitUntil("likes did not settle") { vm.likes == ["p1"] }
        XCTAssertNil(vm.error)
    }

    func testOfflineProgressDoesNotCrashTheReader() async throws {
        let store = FakeReaderStore()
        store.progressError = offlineError()
        let vm = ReaderViewModel(library: store, bookId: readerBook)
        await vm.load()

        vm.saveProgress(2)
        try await waitUntil("progress was not saved") { store.progressCalls == 1 }

        XCTAssertNil(vm.error)
        XCTAssertEqual(vm.posts.map(\.id), ["p1"])
        XCTAssertFalse(vm.loading)
    }

    func testRevertLikeTouchesOnlyThatPost() {
        XCTAssertEqual(revertLike(["p2"], postId: "p1", before: ["p1"]), ["p1", "p2"])
        XCTAssertEqual(revertLike(["p1", "p2"], postId: "p2", before: ["p1"]), ["p1"])
    }
}

@MainActor
private final class FakeReaderStore: LibraryStore {
    var likesError: Error?
    var indexError: Error?
    var feedError: Error?
    var likeError: Error?
    var progressError: Error?
    private(set) var likesStarted = 0
    private(set) var progressCalls = 0
    private var likePark = TestPark()
    private var likesHeld = false

    private let record = BookRecord(
        id: readerBook,
        title: "Pride",
        author: "Austen",
        handle: "austen",
        importedAt: 1,
        postCount: 1,
        progressIndex: 0
    )
    private let posts = [FeedPost(id: "p1", kind: .body, text: "A line.", chapter: "One")]

    var books: [BookRecord] { [record] }
    var loadState: LibraryLoad { .loaded(books: [record], refreshing: false) }

    func armLikes() {
        likePark = TestPark()
        likesHeld = true
    }

    func releaseLikes() {
        likesHeld = false
        likePark.release()
    }

    func load() async throws {}
    func dismissLoadError() {}
    func importFromFile(_ file: URL) async throws -> BookRecord { throw CancellationError() }
    func importSample() async throws -> BookRecord { throw CancellationError() }
    func delete(id: String) async throws {}
    func book(id: String) -> BookRecord? { record.id == id ? record : nil }

    func feed(id: String) async throws -> [FeedPost] {
        if let feedError { throw feedError }
        return posts
    }

    func saveProgress(id: String, index: Int) async throws {
        progressCalls += 1
        if let progressError { throw progressError }
    }

    func likes(id: String) async throws -> Set<String> {
        if let likesError { throw likesError }
        return ["p1"]
    }

    func toggleLike(id: String, postId: String) async throws -> Set<String> {
        likesStarted += 1
        if likesHeld { try await likePark.wait() }
        if let likeError { throw likeError }
        return [postId]
    }

    func setRepost(id: String, postId: String, on: Bool) async throws -> RepostState {
        RepostState(reposts: on ? 1 : 0, reposted: on)
    }

    func commentIndex(id: String) async throws -> CommentIndex {
        if let indexError { throw indexError }
        return CommentIndex(counts: ["p1": 2])
    }

    func comments(id: String, postId: String) async throws -> [PostComment] { [] }
    func addComment(id: String, postId: String, text: String) async throws -> [PostComment] { [] }
    func deleteComment(id: String, postId: String, commentId: String) async throws -> [PostComment] { [] }
    func search(query: String) async throws -> SearchResult { SearchResult() }
    func likedPosts() async throws -> [PostHit] { [] }
    func profileReplies(handle: String) async throws -> [ProfileReply] { [] }
    func coverFile(id: String) -> URL? { nil }
}

private let readerBook = "11111111-1111-1111-1111-111111111111"
