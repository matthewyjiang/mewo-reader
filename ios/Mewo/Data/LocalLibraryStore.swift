import Foundation
import Observation
import UIKit

/// On-device library: JSON metadata, EPUB files, and the feed cache.
@Observable
@MainActor
final class LocalLibraryStore: LibraryStore {
    private let opener: EpubOpener
    private let profile: LocalProfileStore
    private let extractor = FeedExtractor()
    private let root: URL
    private let libraryFile: URL
    private let lock = NSLock()
    private(set) var books: [BookRecord] = []

    init(opener: EpubOpener, profile: LocalProfileStore, files: URL) {
        self.opener = opener
        self.profile = profile
        self.root = files.appendingPathComponent("books", isDirectory: true)
        self.libraryFile = root.appendingPathComponent("library.json")
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }

    func load() async throws {
        books = readLibrary().books
    }

    func importFromFile(_ file: URL) async throws -> BookRecord {
        let id = UUID().uuidString
        let dir = root.appendingPathComponent(id, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent("book.epub")
        let accessed = file.startAccessingSecurityScopedResource()
        defer { if accessed { file.stopAccessingSecurityScopedResource() } }
        if FileManager.default.fileExists(atPath: dest.path) {
            try FileManager.default.removeItem(at: dest)
        }
        try FileManager.default.copyItem(at: file, to: dest)
        return try await ingest(id: id, file: dest, isSample: false)
    }

    func importSample() async throws -> BookRecord {
        if let existing = books.first(where: { $0.isSample }) {
            return existing
        }
        let id = "sample"
        let dir = root.appendingPathComponent(id, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent("book.epub")
        guard let bundled = Bundle.main.url(forResource: "sample", withExtension: "epub") else {
            throw NSError(domain: "mewo", code: 5, userInfo: [NSLocalizedDescriptionKey: "Sample book is missing."])
        }
        if FileManager.default.fileExists(atPath: dest.path) {
            try FileManager.default.removeItem(at: dest)
        }
        try FileManager.default.copyItem(at: bundled, to: dest)
        return try await ingest(id: id, file: dest, isSample: true)
    }

    func delete(id: String) async throws {
        try? FileManager.default.removeItem(at: root.appendingPathComponent(id, isDirectory: true))
        writeLibrary(LibrarySnapshot(books: books.filter { $0.id != id }))
    }

    func book(id: String) -> BookRecord? {
        books.first { $0.id == id }
    }

    func coverFile(id: String) -> URL? {
        let url = root.appendingPathComponent(id).appendingPathComponent("cover.jpg")
        return FileManager.default.fileExists(atPath: url.path) ? url : nil
    }

    func feed(id: String) async throws -> [FeedPost] {
        let cacheFile = root.appendingPathComponent(id).appendingPathComponent("feed.json")
        if let cache = JSONFiles.read(FeedCache.self, from: cacheFile), cache.version == feedCacheVersion {
            return cache.posts
        }
        let epub = root.appendingPathComponent(id).appendingPathComponent("book.epub")
        let posts = try await opener.open(file: epub) { publication in
            try await FeedExtractor().extract(publication)
        }
        try JSONFiles.write(FeedCache(version: feedCacheVersion, posts: posts), to: cacheFile)
        let updated = books.map { book in
            book.id == id ? BookRecord(
                id: book.id, title: book.title, author: book.author, handle: book.handle,
                importedAt: book.importedAt, postCount: posts.count, progressIndex: 0,
                isSample: book.isSample, mine: book.mine
            ) : book
        }
        writeLibrary(LibrarySnapshot(books: updated))
        return posts
    }

    func saveProgress(id: String, index: Int) async throws {
        let updated = books.map { book in
            book.id == id ? BookRecord(
                id: book.id, title: book.title, author: book.author, handle: book.handle,
                importedAt: book.importedAt, postCount: book.postCount,
                progressIndex: max(0, index), isSample: book.isSample, mine: book.mine
            ) : book
        }
        writeLibrary(LibrarySnapshot(books: updated))
    }

    func likes(id: String) async throws -> Set<String> {
        let file = root.appendingPathComponent(id).appendingPathComponent("likes.json")
        return JSONFiles.read(LikeSet.self, from: file)?.ids ?? []
    }

    func toggleLike(id: String, postId: String) async throws -> Set<String> {
        var current = try await likes(id: id)
        if !current.insert(postId).inserted {
            current.remove(postId)
        }
        let file = root.appendingPathComponent(id).appendingPathComponent("likes.json")
        try JSONFiles.write(LikeSet(ids: current), to: file)
        return current
    }

    func commentIndex(id: String) async throws -> CommentIndex {
        let file = readComments(id)
        return CommentIndex(
            counts: file.posts.mapValues(\.count),
            mine: Set(file.posts.filter { !$0.value.isEmpty }.keys)
        )
    }

    func comments(id: String, postId: String) async throws -> [PostComment] {
        localThread(readComments(id).posts[postId] ?? [])
    }

    func addComment(id: String, postId: String, text: String) async throws -> [PostComment] {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            throw NSError(domain: "mewo", code: 6, userInfo: [NSLocalizedDescriptionKey: "Write something first."])
        }
        if trimmed.count > maxCommentChars {
            throw NSError(
                domain: "mewo",
                code: 7,
                userInfo: [NSLocalizedDescriptionKey: "Comment is longer than \(maxCommentChars) characters (asked \(trimmed.count))."]
            )
        }
        var current = readComments(id)
        var next = current.posts
        let row = LocalComment(id: UUID().uuidString, text: trimmed, createdAt: nowMillis())
        next[postId, default: []].append(row)
        try JSONFiles.write(LocalCommentFile(posts: next), to: root.appendingPathComponent(id).appendingPathComponent("comments.json"))
        return localThread(next[postId] ?? [])
    }

    func deleteComment(id: String, postId: String, commentId: String) async throws -> [PostComment] {
        var current = readComments(id)
        var next = current.posts
        next[postId] = next[postId]?.filter { $0.id != commentId }
        if next[postId]?.isEmpty == true { next.removeValue(forKey: postId) }
        try JSONFiles.write(LocalCommentFile(posts: next), to: root.appendingPathComponent(id).appendingPathComponent("comments.json"))
        return localThread(next[postId] ?? [])
    }

    func search(query: String) async throws -> SearchResult {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if q.isEmpty { return SearchResult() }
        if books.isEmpty { books = readLibrary().books }
        let bookHits = books.filter {
            $0.title.localizedCaseInsensitiveContains(q)
                || $0.author.localizedCaseInsensitiveContains(q)
                || $0.handle.localizedCaseInsensitiveContains(q)
        }
        let postHits = books.flatMap { book -> [PostHit] in
            guard let posts = cachedFeed(book.id) else { return [] }
            return posts.filter {
                $0.text.localizedCaseInsensitiveContains(q) || $0.chapter.localizedCaseInsensitiveContains(q)
            }.map { PostHit(book: book, post: $0) }
        }
        return SearchResult(books: bookHits, posts: postHits)
    }

    func likedPosts() async throws -> [PostHit] {
        if books.isEmpty { books = readLibrary().books }
        var hits: [PostHit] = []
        for book in books {
            let ids = try await likes(id: book.id)
            if ids.isEmpty { continue }
            let posts = cachedFeed(book.id) ?? []
            hits.append(contentsOf: posts.filter { ids.contains($0.id) }.map { PostHit(book: book, post: $0) })
        }
        return hits
    }

    func profileReplies(handle: String) async throws -> [ProfileReply] {
        let selfHandle = await MainActor.run { profile.profile.handle }
        if selfHandle.isEmpty || handle.caseInsensitiveCompare(selfHandle) != .orderedSame {
            return []
        }
        if books.isEmpty { books = readLibrary().books }
        var replies: [ProfileReply] = []
        for book in books {
            let file = readComments(book.id)
            if file.posts.isEmpty { continue }
            let posts = await postMap(book.id)
            for (postId, rows) in file.posts {
                guard let post = posts[postId] else { continue }
                replies.append(contentsOf: localThread(rows).map {
                    ProfileReply(book: book, post: post, comment: $0)
                })
            }
        }
        return replies.sorted { $0.comment.createdAt > $1.comment.createdAt }
    }

    private func postMap(_ id: String) async -> [String: FeedPost] {
        let posts: [FeedPost]
        if let cached = cachedFeed(id) {
            posts = cached
        } else {
            posts = (try? await feed(id: id)) ?? []
        }
        return Dictionary(uniqueKeysWithValues: posts.map { ($0.id, $0) })
    }

    private func cachedFeed(_ id: String) -> [FeedPost]? {
        let cacheFile = root.appendingPathComponent(id).appendingPathComponent("feed.json")
        guard let cache = JSONFiles.read(FeedCache.self, from: cacheFile), cache.version == feedCacheVersion else {
            return nil
        }
        return cache.posts
    }

    private func ingest(id: String, file: URL, isSample: Bool) async throws -> BookRecord {
        let meta = try await opener.metadata(file: file)
        writeCover(id: id, cover: meta.cover)
        let record = BookRecord(
            id: id,
            title: meta.title,
            author: meta.author,
            handle: slug(meta.author),
            importedAt: nowMillis(),
            isSample: isSample
        )
        let next = books.filter { $0.id != id } + [record]
        writeLibrary(LibrarySnapshot(books: next.sorted { $0.importedAt > $1.importedAt }))
        return record
    }

    private func writeCover(id: String, cover: UIImage?) {
        guard let cover, let data = cover.jpegData(compressionQuality: 0.85) else { return }
        let url = root.appendingPathComponent(id).appendingPathComponent("cover.jpg")
        try? data.write(to: url, options: .atomic)
    }

    private func readComments(_ id: String) -> LocalCommentFile {
        let file = root.appendingPathComponent(id).appendingPathComponent("comments.json")
        return JSONFiles.read(LocalCommentFile.self, from: file) ?? LocalCommentFile()
    }

    private func localThread(_ rows: [LocalComment]) -> [PostComment] {
        rows.map {
            PostComment(id: $0.id, username: localCommentName, text: $0.text, createdAt: $0.createdAt, mine: true)
        }
    }

    private func readLibrary() -> LibrarySnapshot {
        JSONFiles.read(LibrarySnapshot.self, from: libraryFile) ?? LibrarySnapshot()
    }

    private func writeLibrary(_ snapshot: LibrarySnapshot) {
        try? JSONFiles.write(snapshot, to: libraryFile)
        books = snapshot.books
    }
}
