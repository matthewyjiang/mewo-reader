import Foundation

@MainActor
protocol LibraryStore: AnyObject {
    var books: [BookRecord] { get }

    func load() async throws
    func importFromFile(_ file: URL) async throws -> BookRecord
    func importSample() async throws -> BookRecord
    func delete(id: String) async throws
    func book(id: String) -> BookRecord?
    func feed(id: String) async throws -> [FeedPost]
    func saveProgress(id: String, index: Int) async throws
    func likes(id: String) async throws -> Set<String>
    func toggleLike(id: String, postId: String) async throws -> Set<String>
    func commentIndex(id: String) async throws -> CommentIndex
    func comments(id: String, postId: String) async throws -> [PostComment]
    func addComment(id: String, postId: String, text: String) async throws -> [PostComment]
    func deleteComment(id: String, postId: String, commentId: String) async throws -> [PostComment]
    func search(query: String) async throws -> SearchResult
    func likedPosts() async throws -> [PostHit]
    func profileReplies(handle: String) async throws -> [ProfileReply]
    func coverFile(id: String) -> URL?
}

@MainActor
final class SwitchingLibraryStore: LibraryStore {
    private let local: LocalLibraryStore
    private let hosted: HostedLibraryStore
    private let backend: BackendStore

    init(local: LocalLibraryStore, hosted: HostedLibraryStore, backend: BackendStore) {
        self.local = local
        self.hosted = hosted
        self.backend = backend
    }

    private var active: LibraryStore {
        backend.kind == .hosted ? hosted : local
    }

    var books: [BookRecord] { active.books }

    func load() async throws { try await active.load() }
    func importFromFile(_ file: URL) async throws -> BookRecord { try await active.importFromFile(file) }
    func importSample() async throws -> BookRecord { try await active.importSample() }
    func delete(id: String) async throws { try await active.delete(id: id) }
    func book(id: String) -> BookRecord? { active.book(id: id) }
    func feed(id: String) async throws -> [FeedPost] { try await active.feed(id: id) }
    func saveProgress(id: String, index: Int) async throws { try await active.saveProgress(id: id, index: index) }
    func likes(id: String) async throws -> Set<String> { try await active.likes(id: id) }
    func toggleLike(id: String, postId: String) async throws -> Set<String> {
        try await active.toggleLike(id: id, postId: postId)
    }
    func commentIndex(id: String) async throws -> CommentIndex { try await active.commentIndex(id: id) }
    func comments(id: String, postId: String) async throws -> [PostComment] {
        try await active.comments(id: id, postId: postId)
    }
    func addComment(id: String, postId: String, text: String) async throws -> [PostComment] {
        try await active.addComment(id: id, postId: postId, text: text)
    }
    func deleteComment(id: String, postId: String, commentId: String) async throws -> [PostComment] {
        try await active.deleteComment(id: id, postId: postId, commentId: commentId)
    }
    func search(query: String) async throws -> SearchResult { try await active.search(query: query) }
    func likedPosts() async throws -> [PostHit] { try await active.likedPosts() }
    func profileReplies(handle: String) async throws -> [ProfileReply] {
        try await active.profileReplies(handle: handle)
    }
    func coverFile(id: String) -> URL? { active.coverFile(id: id) }
}
