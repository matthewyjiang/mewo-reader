import Foundation
import Observation
import UIKit

/// Hosted shelf. Talks HTTP, then parses the EPUB on the phone.
@Observable
@MainActor
final class HostedLibraryStore: LibraryStore {
    private let opener: EpubOpener
    private let sessions: HostedSessionStore
    private let client: HostedClient
    private let extractor = FeedExtractor()
    private let root: URL
    private var epoch = 0
    private(set) var books: [BookRecord] = []

    init(opener: EpubOpener, sessions: HostedSessionStore, client: HostedClient, files: URL) {
        self.opener = opener
        self.sessions = sessions
        self.client = client
        self.root = files.appendingPathComponent("hosted", isDirectory: true)
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }

    func wipeIfSignedOut(wasSignedIn: Bool, nowSignedIn: Bool) {
        if wasSignedIn && !nowSignedIn {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            books = []
        }
    }

    func load() async throws {
        let session = await MainActor.run { sessions.session }
        if !session.signedIn {
            books = []
            return
        }
        let start = epoch
        do {
            var remote = try await client.library(session)
            for book in remote.books {
                guard let id = safeBookId(book.id) else { continue }
                _ = try? await client.downloadCover(session, id: id, dest: bookDir(id).appendingPathComponent("cover.jpg"))
            }
            remote.books = remote.books.filter { safeBookId($0.id) != nil }
            if epoch == start {
                books = remote.books
            } else {
                let extras = books.filter { local in remote.books.contains { $0.id == local.id } == false }
                books = (remote.books + extras).sorted { $0.importedAt > $1.importedAt }
            }
        } catch let err as HostedException where err.code == 401 {
            await MainActor.run { sessions.signOut() }
            throw err
        }
    }

    func importFromFile(_ file: URL) async throws -> BookRecord {
        let session = try requireSession()
        let scratch = root.appendingPathComponent("tmp-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: scratch) }
        let epub = scratch.appendingPathComponent("book.epub")
        let accessed = file.startAccessingSecurityScopedResource()
        defer { if accessed { file.stopAccessingSecurityScopedResource() } }
        try FileManager.default.copyItem(at: file, to: epub)
        return try await ingest(session, epub: epub, isSample: false, scratch: scratch)
    }

    func importSample() async throws -> BookRecord {
        let session = try requireSession()
        if let existing = books.first(where: { $0.isSample }) { return existing }
        let scratch = root.appendingPathComponent("tmp-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: scratch) }
        guard let bundled = Bundle.main.url(forResource: "sample", withExtension: "epub") else {
            throw NSError(domain: "mewo", code: 5, userInfo: [NSLocalizedDescriptionKey: "Sample book is missing."])
        }
        let epub = scratch.appendingPathComponent("book.epub")
        try FileManager.default.copyItem(at: bundled, to: epub)
        return try await ingest(session, epub: epub, isSample: true, scratch: scratch)
    }

    func delete(id: String) async throws {
        let session = try requireSession()
        let safe = try requireBookId(id)
        try await expireOn401 { try await client.delete(session, id: safe) }
        try? FileManager.default.removeItem(at: bookDir(safe))
        epoch += 1
        books = books.filter { $0.id != safe }
    }

    func book(id: String) -> BookRecord? { books.first { $0.id == id } }

    func coverFile(id: String) -> URL? {
        guard let safe = safeBookId(id) else { return nil }
        let url = bookDir(safe).appendingPathComponent("cover.jpg")
        return FileManager.default.fileExists(atPath: url.path) ? url : nil
    }

    func feed(id: String) async throws -> [FeedPost] {
        let session = try requireSession()
        let safe = try requireBookId(id)
        let epub = try await ensureEpub(session, id: safe)
        let cacheFile = bookDir(safe).appendingPathComponent("feed.json")
        if let cache = JSONFiles.read(FeedCache.self, from: cacheFile), cache.version == feedCacheVersion {
            return cache.posts
        }
        let posts = try await opener.open(file: epub) { publication in
            try await FeedExtractor().extract(publication)
        }
        try FileManager.default.createDirectory(at: bookDir(safe), withIntermediateDirectories: true)
        try JSONFiles.write(FeedCache(version: feedCacheVersion, posts: posts), to: cacheFile)
        let updated = try await expireOn401 { try await client.patch(session, id: safe, body: PatchBook(postCount: posts.count)) }
        replaceBook(updated)
        return posts
    }

    func saveProgress(id: String, index: Int) async throws {
        let session = try requireSession()
        let safe = try requireBookId(id)
        let updated = try await expireOn401 {
            try await client.patch(session, id: safe, body: PatchBook(progressIndex: max(0, index)))
        }
        replaceBook(updated)
    }

    func likes(id: String) async throws -> Set<String> {
        let safe = try requireBookId(id)
        return try await expireOn401 { try await client.likes(requireSession(), id: safe) }
    }

    func toggleLike(id: String, postId: String) async throws -> Set<String> {
        let safe = try requireBookId(id)
        return try await expireOn401 { try await client.toggleLike(requireSession(), id: safe, postId: postId) }
    }

    func commentIndex(id: String) async throws -> CommentIndex {
        let safe = try requireBookId(id)
        return try await expireOn401 { try await client.commentIndex(requireSession(), id: safe) }
    }

    func comments(id: String, postId: String) async throws -> [PostComment] {
        let safe = try requireBookId(id)
        return try await expireOn401 { try await client.comments(requireSession(), id: safe, postId: postId) }
    }

    func addComment(id: String, postId: String, text: String) async throws -> [PostComment] {
        let safe = try requireBookId(id)
        return try await expireOn401 { try await client.addComment(requireSession(), id: safe, postId: postId, text: text) }
    }

    func deleteComment(id: String, postId: String, commentId: String) async throws -> [PostComment] {
        let safe = try requireBookId(id)
        return try await expireOn401 { try await client.deleteComment(requireSession(), id: safe, postId: postId, commentId: commentId) }
    }

    func search(query: String) async throws -> SearchResult {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if q.isEmpty { return SearchResult() }
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
        let session = await MainActor.run { sessions.session }
        if !session.signedIn { return [] }
        var hits: [PostHit] = []
        for book in books {
            let ids = try await expireOn401 { try await client.likes(session, id: book.id) }
            if ids.isEmpty { continue }
            let posts = cachedFeed(book.id) ?? []
            hits.append(contentsOf: posts.filter { ids.contains($0.id) }.map { PostHit(book: book, post: $0) })
        }
        return hits
    }

    func profileReplies(handle: String) async throws -> [ProfileReply] {
        let session = try requireSession()
        let remote = try await expireOn401 { try await client.profileReplies(session, handle: handle) }
        let byId = Dictionary(uniqueKeysWithValues: books.map { ($0.id, $0) })
        var feeds: [String: [String: FeedPost]] = [:]
        var replies: [ProfileReply] = []
        for row in remote.replies {
            guard let book = byId[row.bookId] else { continue }
            if feeds[book.id] == nil {
                let list: [FeedPost]
                if let cached = cachedFeed(book.id) {
                    list = cached
                } else {
                    list = (try? await feed(id: book.id)) ?? []
                }
                feeds[book.id] = Dictionary(uniqueKeysWithValues: list.map { ($0.id, $0) })
            }
            guard let post = feeds[book.id]?[row.postId] else { continue }
            replies.append(ProfileReply(book: book, post: post, comment: row.comment))
        }
        return replies
    }

    private func ingest(_ session: HostedSession, epub: URL, isSample: Bool, scratch: URL) async throws -> BookRecord {
        let meta = try await opener.metadata(file: epub)
        let cover = scratch.appendingPathComponent("cover.jpg")
        if let image = meta.cover, let data = image.jpegData(compressionQuality: 0.85) {
            try data.write(to: cover, options: .atomic)
        }
        var record = try await expireOn401 {
            try await client.upload(
                session,
                epub: epub,
                cover: FileManager.default.fileExists(atPath: cover.path) ? cover : nil,
                title: meta.title,
                author: meta.author,
                handle: slug(meta.author),
                isSample: isSample
            )
        }
        let safe = try requireBookId(record.id)
        let dest = bookDir(safe)
        try FileManager.default.createDirectory(at: dest, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: epub, to: dest.appendingPathComponent("book.epub"))
        if FileManager.default.fileExists(atPath: cover.path) {
            try? FileManager.default.copyItem(at: cover, to: dest.appendingPathComponent("cover.jpg"))
        }
        record.id = safe
        replaceBook(record)
        return record
    }

    private func ensureEpub(_ session: HostedSession, id: String) async throws -> URL {
        let dest = bookDir(id).appendingPathComponent("book.epub")
        if !FileManager.default.fileExists(atPath: dest.path)
            || ((try? dest.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0) == 0 {
            try await expireOn401 { try await client.downloadEpub(session, id: id, dest: dest) }
        }
        return dest
    }

    private func cachedFeed(_ id: String) -> [FeedPost]? {
        guard let safe = safeBookId(id) else { return nil }
        let cacheFile = bookDir(safe).appendingPathComponent("feed.json")
        guard let cache = JSONFiles.read(FeedCache.self, from: cacheFile), cache.version == feedCacheVersion else {
            return nil
        }
        return cache.posts
    }

    private func replaceBook(_ record: BookRecord) {
        epoch += 1
        let next = books.filter { $0.id != record.id } + [record]
        books = next.sorted { $0.importedAt > $1.importedAt }
    }

    private func requireSession() throws -> HostedSession {
        let session = sessions.session
        if !session.signedIn {
            throw NSError(domain: "mewo", code: 8, userInfo: [NSLocalizedDescriptionKey: hostedNotSignedIn])
        }
        return session
    }

    private func expireOn401<T>(_ block: () async throws -> T) async throws -> T {
        do {
            return try await block()
        } catch let err as HostedException where err.code == 401 {
            await MainActor.run { sessions.signOut() }
            throw err
        }
    }

    private func bookDir(_ id: String) -> URL {
        root.appendingPathComponent(id, isDirectory: true)
    }
}

func safeBookId(_ id: String) -> String? {
    UUID(uuidString: id)?.uuidString.lowercased() == id.lowercased() ? id : nil
}

func requireBookId(_ id: String) throws -> String {
    guard let safe = safeBookId(id) else {
        throw NSError(domain: "mewo", code: 9, userInfo: [NSLocalizedDescriptionKey: "Bad book id."])
    }
    return safe
}

final class HostedAuth {
    private let sessions: HostedSessionStore
    private let client: HostedClient

    init(sessions: HostedSessionStore, client: HostedClient) {
        self.sessions = sessions
        self.client = client
    }

    func signIn(username: String, password: String) async throws {
        let url = try requireUrl()
        let res = try await client.login(baseUrl: url, username: username.trimmingCharacters(in: .whitespacesAndNewlines), password: password)
        await MainActor.run { sessions.setSignedIn(username: res.username, token: res.token) }
    }

    func createAccount(username: String, password: String) async throws {
        let url = try requireUrl()
        let res = try await client.register(baseUrl: url, username: username.trimmingCharacters(in: .whitespacesAndNewlines), password: password)
        await MainActor.run { sessions.setSignedIn(username: res.username, token: res.token) }
    }

    func signOut() async {
        let session = await MainActor.run { sessions.session }
        await MainActor.run { sessions.signOut() }
        if session.signedIn {
            try? await client.logout(session)
        }
    }

    private func requireUrl() throws -> String {
        let url = sessions.session.normalizedUrl()
        guard let url, !url.isEmpty else {
            throw NSError(domain: "mewo", code: 10, userInfo: [NSLocalizedDescriptionKey: "Set a server URL first."])
        }
        return url
    }
}
