import Foundation

@MainActor
protocol LibraryStore: AnyObject {
    var books: [BookRecord] { get }
    /// Loading, loaded (optionally refreshing), or failed with an optional saved shelf.
    var loadState: LibraryLoad { get }

    func load() async throws
    /// Drop a failed-with-cache banner once the reader has dismissed it.
    func dismissLoadError()
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
    var loadState: LibraryLoad { active.loadState }

    func load() async throws { try await active.load() }
    func dismissLoadError() { active.dismissLoadError() }
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

/// One book and the cached feed file a search reads for it. Stores build these
/// on the main actor so the scan can run without touching the store.
struct SearchSource: Sendable {
    var book: BookRecord
    /// Nil when the book has no safe cache path.
    var feedFile: URL?
    /// The `FeedCache.version` this store accepts.
    var cacheVersion: Int
}

/// Matches books by title, author, or handle, and cached posts by text or
/// chapter. Runs off the main actor and checks for cancellation between books
/// and posts, so a newer keystroke stops it.
@concurrent
func scanSearch(query: String, sources: [SearchSource]) async throws -> SearchResult {
    let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
    if q.isEmpty { return SearchResult() }
    let books = sources.map(\.book).filter {
        $0.title.localizedCaseInsensitiveContains(q)
            || $0.author.localizedCaseInsensitiveContains(q)
            || $0.handle.localizedCaseInsensitiveContains(q)
    }
    var posts: [PostHit] = []
    for source in sources {
        try Task.checkCancellation()
        guard let file = source.feedFile,
              let cache = JSONFiles.read(FeedCache.self, from: file),
              cache.version == source.cacheVersion else { continue }
        for post in cache.posts {
            try Task.checkCancellation()
            if post.text.localizedCaseInsensitiveContains(q) || post.chapter.localizedCaseInsensitiveContains(q) {
                posts.append(PostHit(book: source.book, post: post))
            }
        }
    }
    return SearchResult(books: books, posts: posts)
}
