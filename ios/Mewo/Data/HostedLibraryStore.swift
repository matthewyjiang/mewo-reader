import Foundation
import Observation
import UIKit

/// Hosted shelf. Talks HTTP. The server parses each book, so every client
/// reads the same posts and post ids. Readium here only reads title, author,
/// and cover for upload.
@Observable
@MainActor
final class HostedLibraryStore: LibraryStore {
    private let opener: EpubOpener
    private let sessions: HostedSessionStore
    private let client: HostedClient
    private let root: URL
    private let shelfCache: ShelfCache
    private var epoch = 0
    private var flight: RefreshFlight?
    /// Rows added by an import while a refresh is in flight. A refresh keeps only these, not every local row the server dropped.
    private var importedWhileRefreshing: Set<String> = []
    /// Ids deleted while a refresh is in flight. The reply already on the way must not put them back.
    private var removedWhileRefreshing: Set<String> = []
    private var shareConfirmed: [String: Bool] = [:]
    private var shareTails: [String: Task<Void, Never>] = [:]
    private var shareTokens: [String: UUID] = [:]
    private(set) var books: [BookRecord] = []
    private(set) var loadState: LibraryLoad = .loading
    private(set) var offline: [String: OfflineState] = [:]
    private(set) var offlineSummary = OfflineSummary(downloaded: 0, total: 0, running: false)
    /// In-flight download marks. Downloaded and notDownloaded are derived in `publishOffline`.
    private var live: [String: OfflineState] = [:]
    private var liveGeneration: [String: Int] = [:]
    private var onDisk: Set<String> = []
    private var stalePosts: Set<String> = []
    private var bookJobs: [String: DownloadJob] = [:]
    private var feedTasks: [UUID: Task<[FeedPost], Error>] = [:]
    private var batch: DownloadJob?
    private var maintenance: Task<Void, Never>?
    /// Token check and hosted-file writes share this lock with the sign-out wipe.
    private let files = HostedFileGate()

    init(
        opener: EpubOpener,
        sessions: HostedSessionStore,
        client: HostedClient,
        files: URL,
        refreshesOnInit: Bool = true
    ) {
        self.opener = opener
        self.sessions = sessions
        self.client = client
        self.root = files.appendingPathComponent("hosted", isDirectory: true)
        self.shelfCache = ShelfCache(directory: files.appendingPathComponent("hosted-shelves", isDirectory: true))
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        self.files.setToken(sessions.session.token)
        sessions.onSessionChange = { [weak self] previous, next in
            self?.sessionChanged(from: previous, to: next)
        }
        restoreCachedShelf()
        // A local backend still builds this store. It must not hit the network until a hosted load.
        if refreshesOnInit {
            Task { try? await self.load() }
        } else if case .loaded(let cached, true) = loadState {
            loadState = .loaded(books: cached, refreshing: false)
        }
    }

    /// A pull during launch waits on the refresh already running for this token.
    /// Callers only wait. SwiftUI cancels a pull and a reader task when those views go away;
    /// that must not cancel the shared refresh. A session change is what cancels it.
    func load() async throws {
        let token = sessions.session.token
        if let flight, flight.token == token {
            try await wait(flight.task)
            return
        }
        let id = UUID()
        let task = Task { @MainActor in
            defer {
                if self.flight?.id == id {
                    self.flight = nil
                }
            }
            try await self.loadBody(startedWith: token)
        }
        let mine = RefreshFlight(id: id, task: task, token: token)
        let previous = flight
        flight = mine
        previous?.task.cancel()
        // `task.value` cancels the task when the waiter is cancelled. A pull must not do that.
        try await wait(task)
    }

    func dismissLoadError() {
        loadState = reduceLibraryLoad(loadState, .dismissed)
    }

    private func loadBody(startedWith token: String?) async throws {
        try Task.checkCancellation()
        let session = sessions.session
        // The flight token is the one captured when this refresh started. A sign-in that
        // landed while this task was queued must not fetch with the new account's session.
        guard session.token == token else { return }
        if !session.signedIn {
            books = []
            loadState = .loaded(books: [], refreshing: false)
            return
        }
        guard let url = session.normalizedUrl() else { return }
        let user = session.username ?? ""
        let cached = shelfCache.read(serverUrl: url, username: user)
        guard sessions.session.token == session.token else { return }
        if let cached, books.isEmpty {
            books = cached
            files.setShelf(Set(cached.map(\.id)))
            loadState = reduceLibraryLoad(loadState, .cached(cached))
        }
        loadState = reduceLibraryLoad(loadState, .refreshStarted)
        importedWhileRefreshing.removeAll()
        removedWhileRefreshing.removeAll()
        // A failed refresh must still mark books whose posts.json is already on disk.
        await rescan(books)
        let start = epoch
        do {
            let remote = try await fetchLibrary(session)
            guard sessions.session.token == session.token else { return }
            let serverBooks = remote.books.filter { removedWhileRefreshing.contains($0.id) == false }
            let merged: [BookRecord]
            if epoch == start {
                merged = serverBooks
            } else {
                let extras = books.filter { local in
                    importedWhileRefreshing.contains(local.id)
                        && removedWhileRefreshing.contains(local.id) == false
                        && serverBooks.contains { $0.id == local.id } == false
                }
                merged = (serverBooks + extras).sorted { $0.importedAt > $1.importedAt }
            }
            let shown = keepingInFlightShare(merged)
            await publish(shown, .refreshSucceeded(shown), owner: session)
        } catch is CancellationError {
            throw CancellationError()
        } catch let url as URLError where url.code == .cancelled {
            throw CancellationError()
        } catch {
            if sessions.session.token == session.token {
                let message = error.localizedDescription
                loadState = reduceLibraryLoad(
                    loadState,
                    .refreshFailed(message.isEmpty ? "Could not load the library." : message)
                )
            }
            throw error
        }
    }

    private func fetchLibrary(_ session: HostedSession) async throws -> LibrarySnapshot {
        do {
            var remote = try await client.library(session)
            for book in remote.books {
                guard let id = safeBookId(book.id) else { continue }
                _ = try await expireOn401(session) {
                    try await client.downloadCover(session, id: id, dest: bookDir(id).appendingPathComponent("cover.jpg"))
                }
            }
            remote.books = remote.books.filter { safeBookId($0.id) != nil }
            return remote
        } catch let err as HostedException where err.code == 401 {
            if sessions.session.token == session.token {
                sessions.signOut()
            }
            throw err
        }
    }

    /// Any token change drops the shelf that was on screen before the next load paints.
    /// Signing in with nothing saved stays `.loading`, so the empty copy does not flash first.
    /// File wipes wait until in-flight downloads and reader loads have stopped.
    private func sessionChanged(from previous: HostedSession, to next: HostedSession) {
        files.setToken(next.token)
        let urlChanged = previous.normalizedUrl() != next.normalizedUrl()
        let userChanged = previous.username != next.username
        let signedOut = previous.signedIn && !next.signedIn
        let tokenChanged = previous.token != next.token
        let reset = tokenChanged || (previous.signedIn && (urlChanged || userChanged))
        if reset {
            flight?.task.cancel()
            books = []
            live.removeAll()
            onDisk.removeAll()
            stalePosts.removeAll()
            publishOffline()
            if next.signedIn {
                restoreCachedShelf()
            } else {
                loadState = .loaded(books: [], refreshing: false)
                files.setShelf([])
            }
        }
        let dropShelf = previous.signedIn && (signedOut || urlChanged || userChanged)
        let wipe = signedOut || (previous.signedIn && urlChanged)
        let clearSocial = previous.signedIn && userChanged && !wipe
        let prior = maintenance
        let task = Task { @MainActor in
            await prior?.value
            if dropShelf {
                await self.clearShelf(previous)
            }
            if wipe || clearSocial {
                await self.cancelTransfers()
            }
            if wipe {
                await self.wipeHostedFiles()
            } else if clearSocial {
                await self.clearSocialFiles()
            }
            try? await self.load()
        }
        maintenance = task
    }

    private func restoreCachedShelf() {
        let session = sessions.session
        guard session.signedIn, let url = session.normalizedUrl(), let user = session.username, !user.isEmpty else {
            loadState = session.signedIn ? .loading : .loaded(books: [], refreshing: false)
            return
        }
        if let cached = shelfCache.read(serverUrl: url, username: user) {
            books = cached
            files.setShelf(Set(cached.map(\.id)))
            loadState = .loaded(books: cached, refreshing: true)
        } else {
            books = []
            files.setShelf([])
            loadState = .loading
        }
    }

    private func clearShelf(_ session: HostedSession) async {
        guard let url = session.normalizedUrl(), let user = session.username else { return }
        let cache = shelfCache
        await offMain { cache.clear(serverUrl: url, username: user) }
    }

    private func wipeHostedFiles() async {
        let root = self.root
        let gate = files
        await offMain {
            gate.sync { _, _ in
                try? FileManager.default.removeItem(at: root)
                try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            }
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
        if let job = bookJobs[safe] {
            job.cancel()
            await waitUntilDone(job.task)
        }
        try await expireOn401(session) { try await client.delete(session, id: safe) }
        let dir = bookDir(safe)
        await offMain { try? FileManager.default.removeItem(at: dir) }
        onDisk.remove(safe)
        stalePosts.remove(safe)
        live[safe] = nil
        publishOffline()
        epoch += 1
        removedWhileRefreshing.insert(safe)
        let next = books.filter { $0.id != safe }
        await publish(next, .booksReplaced(next), owner: session)
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
        guard savedTextAllowed(loadState, bookId: safe) else {
            throw NSError(
                domain: "mewo",
                code: 12,
                userInfo: [NSLocalizedDescriptionKey: "That book is not on your shelf."]
            )
        }
        if let cached = await readCached(safe) { return cached }
        let key = UUID()
        let task = Task { @MainActor () throws -> [FeedPost] in
            let posts = try await self.expireOn401(session) { try await self.client.posts(session, id: safe) }
            try Task.checkCancellation()
            _ = await self.writeCachedPosts(safe, posts: posts, session: session, onShelf: false)
            return posts
        }
        feedTasks[key] = task
        defer { feedTasks[key] = nil }
        return try await withTaskCancellationHandler {
            try await task.value
        } onCancel: {
            task.cancel()
        }
    }

    func setShared(id: String, shared: Bool) async throws {
        let session = try requireSession()
        let safe = try requireBookId(id)
        guard let book = books.first(where: { $0.id == safe }) else { return }
        let pending = shareTails[safe] == nil ? nil : ShareToggle(
            confirmed: shareConfirmed[safe] ?? book.shared,
            displayed: book.shared
        )
        switch startShareToggle(book: book, requested: shared, pending: pending) {
        case .ignore:
            return
        case .forbidden:
            throw NSError(
                domain: "mewo",
                code: 11,
                userInfo: [NSLocalizedDescriptionKey: "Only the person who added this book can change sharing."]
            )
        case .update(let toggle):
            if shareConfirmed[safe] == nil {
                shareConfirmed[safe] = toggle.confirmed
            }
            if book.shared != toggle.displayed {
                var next = book
                next.shared = toggle.displayed
                await replaceBook(next, owner: session)
            }
        }

        let previous = shareTails[safe]
        let token = UUID()
        shareTokens[safe] = token
        let task = Task { @MainActor () throws -> Void in
            if let previous {
                _ = await previous.value
            }
            do {
                let updated = try await self.expireOn401(session) {
                    try await self.client.patch(session, id: safe, body: PatchBook(shared: shared))
                }
                await self.finishShare(id: safe, requested: shared, server: updated, failed: false, owner: session)
            } catch {
                await self.finishShare(id: safe, requested: shared, server: nil, failed: true, owner: session)
                throw error
            }
        }
        shareTails[safe] = Task { @MainActor in
            _ = try? await task.value
        }
        do {
            try await task.value
        } catch {
            clearShareTail(safe, token: token)
            throw error
        }
        clearShareTail(safe, token: token)
    }

    func saveProgress(id: String, index: Int) async throws {
        let session = try requireSession()
        let safe = try requireBookId(id)
        let updated = try await expireOn401(session) {
            try await client.patch(session, id: safe, body: PatchBook(progressIndex: max(0, index)))
        }
        var record = updated
        if shareTails[safe] != nil, let shown = books.first(where: { $0.id == safe })?.shared {
            record.shared = shown
        }
        await replaceBook(record, owner: session)
    }

    func likes(id: String) async throws -> Set<String> {
        let safe = try requireBookId(id)
        let session = try requireSession()
        do {
            let likes = try await expireOn401(session) { try await client.likes(session, id: safe) }
            await updateSocial(safe, session: session) { cache in
                var next = cache
                next.likes = likes
                return next
            }
            return likes
        } catch let error as URLError {
            if let cached = await readSocial(safe) { return cached.likes }
            throw error
        }
    }

    func toggleLike(id: String, postId: String) async throws -> Set<String> {
        let session = try requireSession()
        let safe = try requireBookId(id)
        return try await expireOn401(session) { try await client.toggleLike(session, id: safe, postId: postId) }
    }

    func setRepost(id: String, postId: String, on: Bool) async throws -> RepostState {
        let session = try requireSession()
        let safe = try requireBookId(id)
        return try await expireOn401(session) {
            try await client.setRepost(session, id: safe, postId: postId, on: on)
        }
    }

    func commentIndex(id: String) async throws -> CommentIndex {
        let session = try requireSession()
        let safe = try requireBookId(id)
        do {
            let index = try await expireOn401(session) { try await client.commentIndex(session, id: safe) }
            await updateSocial(safe, session: session) { cache in
                var next = cache
                next.index = index
                return next
            }
            return index
        } catch let error as URLError {
            if let cached = await readSocial(safe) { return cached.index }
            throw error
        }
    }

    func comments(id: String, postId: String) async throws -> [PostComment] {
        let session = try requireSession()
        let safe = try requireBookId(id)
        do {
            let thread = try await expireOn401(session) { try await client.comments(session, id: safe, postId: postId) }
            await updateSocial(safe, session: session) { cache in
                var next = cache
                next.threads[postId] = thread
                next.index = next.index.afterThread(postId: postId, comments: thread)
                return next
            }
            return thread
        } catch let error as URLError {
            if let thread = await readSocial(safe)?.threads[postId] { return thread }
            throw NSError(domain: "mewo", code: 13, userInfo: [NSLocalizedDescriptionKey: "You're offline."])
        }
    }

    func setDownloaded(id: String, on: Bool) async {
        await changeDownload(id, on: on, fromBatch: false)
    }

    /// Downloads every shelf book that is not downloaded yet, two at a time, in shelf order.
    /// A second call while that run is going cancels it.
    func downloadAll() async {
        if let current = batch, !current.isFinished {
            current.cancel()
            await waitUntilDone(current.task)
            return
        }
        let pending = PreparedDownload { job in
            await self.runDownloadAll(owner: job)
        }
        batch = pending.job
        publishOffline()
        pending.start()
        // The caller's task dies with the screen. The batch does not.
        _ = await cancelledWhileJoining(pending.job.task)
    }

    func addComment(id: String, postId: String, text: String) async throws -> [PostComment] {
        let session = try requireSession()
        let safe = try requireBookId(id)
        return try await expireOn401(session) { try await client.addComment(session, id: safe, postId: postId, text: text) }
    }

    func deleteComment(id: String, postId: String, commentId: String) async throws -> [PostComment] {
        let session = try requireSession()
        let safe = try requireBookId(id)
        return try await expireOn401(session) { try await client.deleteComment(session, id: safe, postId: postId, commentId: commentId) }
    }

    func search(query: String) async throws -> SearchResult {
        let sources = books.map { book in
            SearchSource(
                book: book,
                feedFile: safeBookId(book.id).map { bookDir($0).appendingPathComponent(hostedPostsCache) },
                cacheVersion: hostedPostsCacheVersion
            )
        }
        return try await scanSearch(query: query, sources: sources)
    }

    func likedPosts() async throws -> [PostHit] {
        let session = await MainActor.run { sessions.session }
        if !session.signedIn { return [] }
        var hits: [PostHit] = []
        for book in books {
            let ids = try await expireOn401(session) { try await client.likes(session, id: book.id) }
            if ids.isEmpty { continue }
            let posts = await readCached(book.id) ?? []
            hits.append(contentsOf: posts.filter { ids.contains($0.id) }.map { PostHit(book: book, post: $0) })
        }
        return hits
    }

    func profileReplies(handle: String) async throws -> [ProfileReply] {
        let session = try requireSession()
        let remote = try await expireOn401(session) { try await client.profileReplies(session, handle: handle) }
        let byId = Dictionary(uniqueKeysWithValues: books.map { ($0.id, $0) })
        var feeds: [String: [String: FeedPost]] = [:]
        var replies: [ProfileReply] = []
        for row in remote.replies {
            guard let book = byId[row.bookId] else { continue }
            if feeds[book.id] == nil {
                let list: [FeedPost]
                if let cached = await readCached(book.id) {
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
        var record = try await expireOn401(session) {
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
        // The upload already succeeded. A cache directory that cannot be created must not fail it.
        try? FileManager.default.createDirectory(at: dest, withIntermediateDirectories: true)
        if FileManager.default.fileExists(atPath: cover.path) {
            try? FileManager.default.copyItem(at: cover, to: dest.appendingPathComponent("cover.jpg"))
        }
        record.id = safe
        await replaceBook(record, owner: session)
        return record
    }

    private func readCached(_ id: String) async -> [FeedPost]? {
        guard let safe = safeBookId(id) else { return nil }
        let root = self.root
        return await offMain { readHostedPosts(root: root, id: safe)?.posts }
    }

    private func finishShare(id: String, requested: Bool, server: BookRecord?, failed: Bool, owner: HostedSession) async {
        guard let current = books.first(where: { $0.id == id }) else { return }
        if failed {
            if let confirmed = shareConfirmed[id],
               let rolled = applyShareFailure(book: current, requested: requested, confirmed: confirmed) {
                await replaceBook(rolled, owner: owner)
            }
            return
        }
        guard let server else { return }
        shareConfirmed[id] = server.shared
        if let merged = applyShareSuccess(book: current, requested: requested, server: server) {
            await replaceBook(merged, owner: owner)
        }
    }

    private func clearShareTail(_ id: String, token: UUID) {
        guard shareTokens[id] == token else { return }
        shareTails[id] = nil
        shareTokens[id] = nil
        shareConfirmed[id] = nil
    }

    /// A refresh that lands mid-toggle must not paint over the switch.
    private func keepingInFlightShare(_ incoming: [BookRecord]) -> [BookRecord] {
        guard !shareTails.isEmpty else { return incoming }
        return incoming.map { book in
            guard shareTails[book.id] != nil,
                  let shown = books.first(where: { $0.id == book.id })?.shared,
                  shown != book.shared else { return book }
            var copy = book
            copy.shared = shown
            return copy
        }
    }

    private func replaceBook(_ record: BookRecord, owner: HostedSession? = nil) async {
        let owner = owner ?? sessions.session
        guard sessions.session.token == owner.token else { return }
        epoch += 1
        if books.contains(where: { $0.id == record.id }) == false {
            importedWhileRefreshing.insert(record.id)
        }
        let next = (books.filter { $0.id != record.id } + [record]).sorted { $0.importedAt > $1.importedAt }
        await publish(next, .booksReplaced(next), owner: owner)
    }

    /// Updates the on-screen shelf only while `owner` is still signed in. A cache write failure stays a success.
    private func publish(_ next: [BookRecord], _ event: LibraryEvent, owner: HostedSession) async {
        let stillHere = sessions.session.token == owner.token
        var dropped: [String] = []
        if stillHere {
            let previous = books
            books = next
            files.setShelf(Set(next.map(\.id)))
            loadState = reduceLibraryLoad(loadState, event)
            if case .refreshSucceeded = event {
                let kept = Set(next.map(\.id))
                dropped = previous.compactMap { book in
                    guard kept.contains(book.id) == false else { return nil }
                    return safeBookId(book.id)
                }
            }
        }
        if stillHere, owner.signedIn, let url = owner.normalizedUrl(), let user = owner.username, !user.isEmpty {
            switch event {
            case .booksReplaced, .refreshSucceeded:
                let cache = shelfCache
                let snapshot = next
                await offMain {
                    // A full disk must not turn a fetched shelf into an error.
                    try? cache.write(serverUrl: url, username: user, books: snapshot)
                }
            default:
                break
            }
        }
        if !dropped.isEmpty {
            await forgetFiles(dropped)
            for id in dropped {
                onDisk.remove(id)
                stalePosts.remove(id)
            }
        }
        if stillHere {
            await rescan(books)
        }
    }

    private func forgetFiles(_ ids: [String]) async {
        let root = self.root
        let gate = files
        await offMain {
            gate.sync { _, _ in
                for id in ids {
                    let dir = root.appendingPathComponent(id, isDirectory: true)
                    try? FileManager.default.removeItem(at: dir.appendingPathComponent(hostedPostsCache))
                    try? FileManager.default.removeItem(at: dir.appendingPathComponent(hostedSocialFile))
                }
            }
        }
    }

    private func requireSession() throws -> HostedSession {
        let session = sessions.session
        if !session.signedIn {
            throw NSError(domain: "mewo", code: 8, userInfo: [NSLocalizedDescriptionKey: hostedNotSignedIn])
        }
        return session
    }

    private func expireOn401<T>(_ session: HostedSession, _ block: () async throws -> T) async throws -> T {
        // The instance method shares this name, so the call has to name the module.
        try await Mewo.expireOn401(sessions: sessions, session: session, operation: block)
    }

    private func bookDir(_ id: String) -> URL {
        root.appendingPathComponent(id, isDirectory: true)
    }

    /// The caller's task dies on navigation. The download does not.
    /// A Download all worker owns the job it started, and cancels that job if the worker
    /// itself is cancelled. A row download the pass only joined keeps running.
    private func changeDownload(_ id: String, on: Bool, fromBatch: Bool) async {
        guard let safe = safeBookId(id) else { return }
        if !on {
            if let job = bookJobs[safe] {
                job.cancel()
                await waitUntilDone(job.task)
            }
            await removeDownloadFiles(safe)
            onDisk.remove(safe)
            stalePosts.remove(safe)
            live[safe] = nil
            publishOffline()
            return
        }
        if await postsAreCurrent(safe) {
            onDisk.insert(safe)
            if case .failed = live[safe] { live[safe] = nil }
            publishOffline()
            return
        }
        if fromBatch, let beforeClaim = HostedOfflineProbe.beforeClaim {
            await beforeClaim()
        }
        let claim = await claimDownload(safe)
        let cancelled = await cancelledWhileJoining(claim.job.task)
        if cancelled && fromBatch && claim.started {
            claim.job.cancel()
            await waitUntilDone(claim.job.task)
        }
    }

    /// One live download per book. A second caller joins that job.
    /// A job that is cancelled or already finished is awaited, then the slot is claimed again.
    private func claimDownload(_ id: String) async -> DownloadClaim {
        while true {
            if let existing = bookJobs[id] {
                if existing.task.isCancelled || existing.isFinished {
                    await waitUntilDone(existing.task)
                    if bookJobs[id] === existing {
                        bookJobs[id] = nil
                    }
                    continue
                }
                return DownloadClaim(job: existing, started: false)
            }
            let pending = PreparedDownload { job in
                defer {
                    job.markFinished()
                    if self.bookJobs[id] === job {
                        self.bookJobs[id] = nil
                    }
                }
                guard !Task.isCancelled else { return }
                await self.downloadBody(id)
            }
            bookJobs[id] = pending.job
            pending.start()
            return DownloadClaim(job: pending.job, started: true)
        }
    }

    private func runDownloadAll(owner: DownloadJob) async {
        defer {
            owner.markFinished()
            if batch === owner { batch = nil }
            publishOffline()
        }
        publishOffline()
        let ids = books.map(\.id)
        let pending = await idsNeedingDownload(ids)
        await withTaskGroup(of: Void.self) { group in
            var queue = pending
            var running = 0
            func fill() {
                while running < 2, !queue.isEmpty, !Task.isCancelled {
                    let next = queue.removeFirst()
                    running += 1
                    group.addTask { @MainActor in
                        await self.changeDownload(next, on: true, fromBatch: true)
                    }
                }
            }
            fill()
            while !Task.isCancelled, await group.next() != nil {
                running -= 1
                fill()
            }
        }
    }

    private func downloadBody(_ id: String) async {
        let generation = beginLive(id)
        setDownloading(id, nil, generation: generation)
        do {
            let session = try requireSession()
            let posts = try await expireOn401(session) {
                try await client.posts(session, id: id) { [weak self] fraction in
                    self?.reportProgress(id, fraction: fraction, generation: generation)
                }
            }
            try Task.checkCancellation()
            let cover = bookDir(id).appendingPathComponent("cover.jpg")
            _ = try await expireOn401(session) {
                try await client.downloadCover(session, id: id, dest: cover)
            }
            let likes = try await expireOn401(session) { try await client.likes(session, id: id) }
            let index = try await expireOn401(session) { try await client.commentIndex(session, id: id) }
            try Task.checkCancellation()
            let wrote = await writeCachedPosts(id, posts: posts, session: session, onShelf: true)
            guard wrote else {
                endLive(id, nil, generation: generation)
                return
            }
            await updateSocial(id, session: session) { cache in
                var next = cache
                next.likes = likes
                next.index = index
                return next
            }
            endLive(id, nil, generation: generation)
        } catch {
            if isCancellation(error) || Task.isCancelled {
                endLive(id, nil, generation: generation)
                return
            }
            endLive(id, .failed(errorText(error)), generation: generation)
        }
    }

    /// Writes posts.json only while `session` is still current, under the same lock as the wipe.
    /// Returns false when the file was left untouched.
    private func writeCachedPosts(
        _ id: String,
        posts: [FeedPost],
        session: HostedSession,
        onShelf: Bool
    ) async -> Bool {
        let root = self.root
        let gate = files
        let token = session.token
        let wrote = await offMain { () -> Bool in
            gate.sync { current, shelf in
                guard current == token else { return false }
                if onShelf && shelf.contains(id) == false { return false }
                writePostsFile(root: root, id: id, posts: posts)
                return true
            }
        }
        guard wrote else { return false }
        onDisk.insert(id)
        stalePosts.remove(id)
        if case .failed = live[id] { live[id] = nil }
        publishOffline()
        if var record = books.first(where: { $0.id == id }), record.postCount != posts.count {
            record.postCount = posts.count
            await replaceBook(record, owner: session)
        }
        return true
    }

    private func postsAreCurrent(_ id: String) async -> Bool {
        let stale = stalePosts.contains(id)
        let root = self.root
        let found = await offMain { () -> PostsFile in
            inspectPosts(root: root, id: id, knownStale: stale)
        }
        switch found {
        case .missing:
            stalePosts.remove(id)
            return false
        case .mismatch:
            stalePosts.insert(id)
            return false
        case .current:
            stalePosts.remove(id)
            return true
        }
    }

    private func idsNeedingDownload(_ ids: [String]) async -> [String] {
        var pending: [String] = []
        for id in ids {
            guard safeBookId(id) != nil else { continue }
            if await postsAreCurrent(id) {
                onDisk.insert(id)
            } else {
                pending.append(id)
            }
        }
        return pending
    }

    /// Parses a posts file once, the first time it is seen. Later saves only check the set.
    private func rescan(_ books: [BookRecord]) async {
        let ids = Set(books.map(\.id))
        let known = onDisk
        let stale = stalePosts
        let root = self.root
        let result = await offMain { () -> PostsScan in
            var nextStale = stale
            for id in stale {
                let exists = postsFileExists(root: root, id: id)
                if ids.contains(id) == false || !exists { nextStale.remove(id) }
            }
            let gone = known.filter { ids.contains($0) == false || !postsFileExists(root: root, id: $0) }
            var present: [String] = []
            for id in ids where known.contains(id) == false && nextStale.contains(id) == false {
                switch inspectPosts(root: root, id: id, knownStale: false) {
                case .current:
                    present.append(id)
                    nextStale.remove(id)
                case .mismatch:
                    nextStale.insert(id)
                case .missing:
                    nextStale.remove(id)
                }
            }
            return PostsScan(gone: Array(gone), present: present, stale: nextStale)
        }
        for id in result.gone { onDisk.remove(id) }
        for id in result.present { onDisk.insert(id) }
        stalePosts = result.stale
        live = live.filter { ids.contains($0.key) }
        publishOffline()
    }

    private func publishOffline() {
        let marks = live
        let running = batch.map { !$0.isFinished } ?? false
        var map: [String: OfflineState] = [:]
        map.reserveCapacity(books.count)
        var downloaded = 0
        for book in books {
            let state: OfflineState
            switch marks[book.id] {
            case .downloading, .failed:
                state = marks[book.id] ?? .notDownloaded
            default:
                state = onDisk.contains(book.id) ? .downloaded : .notDownloaded
            }
            if case .downloaded = state { downloaded += 1 }
            map[book.id] = state
        }
        offline = map
        offlineSummary = OfflineSummary(downloaded: downloaded, total: books.count, running: running)
    }

    private func beginLive(_ id: String) -> Int {
        let next = (liveGeneration[id] ?? 0) + 1
        liveGeneration[id] = next
        return next
    }

    private func setDownloading(_ id: String, _ fraction: Double?, generation: Int) {
        guard liveGeneration[id] == generation else { return }
        live[id] = .downloading(fraction)
        publishOffline()
    }

    private func endLive(_ id: String, _ state: OfflineState?, generation: Int) {
        liveGeneration[id] = generation + 1
        if let state {
            live[id] = state
        } else {
            live[id] = nil
        }
        publishOffline()
    }

    /// Progress arrives on the URLSession queue. Hop without awaiting so a finish cannot pass it.
    nonisolated private func reportProgress(_ id: String, fraction: Double?, generation: Int) {
        let apply = { @MainActor in
            self.setDownloading(id, fraction, generation: generation)
        }
        if Thread.isMainThread {
            MainActor.assumeIsolated(apply)
        } else {
            DispatchQueue.main.sync {
                MainActor.assumeIsolated(apply)
            }
        }
    }

    private func removeDownloadFiles(_ id: String) async {
        let root = self.root
        let gate = files
        await offMain {
            gate.sync { _, _ in
                let dir = root.appendingPathComponent(id, isDirectory: true)
                try? FileManager.default.removeItem(at: dir.appendingPathComponent(hostedPostsCache))
                try? FileManager.default.removeItem(at: dir.appendingPathComponent(hostedSocialFile))
            }
        }
    }

    private func readSocial(_ id: String) async -> SocialCache? {
        let root = self.root
        return await offMain { readSocialFile(root: root, id: id) }
    }

    private func updateSocial(_ id: String, session: HostedSession, _ update: @escaping @Sendable (SocialCache) -> SocialCache) async {
        let root = self.root
        let gate = files
        let token = session.token
        await offMain {
            gate.sync { current, _ in
                guard current == token else { return }
                let next = update(readSocialFile(root: root, id: id) ?? SocialCache())
                let dir = root.appendingPathComponent(id, isDirectory: true)
                try? JSONFiles.write(next, to: dir.appendingPathComponent(hostedSocialFile))
            }
        }
    }

    private func clearSocialFiles() async {
        let root = self.root
        let gate = files
        await offMain {
            gate.sync { _, _ in
                let dirs = (try? FileManager.default.contentsOfDirectory(
                    at: root,
                    includingPropertiesForKeys: [.isDirectoryKey]
                )) ?? []
                for dir in dirs {
                    try? FileManager.default.removeItem(at: dir.appendingPathComponent(hostedSocialFile))
                }
            }
        }
    }

    private func cancelTransfers() async {
        let running = batch
        let jobs = Array(bookJobs.values)
        let feeds = Array(feedTasks.values)
        running?.cancel()
        jobs.forEach { $0.cancel() }
        feeds.forEach { $0.cancel() }
        if let running { await waitUntilDone(running.task) }
        for job in jobs { await waitUntilDone(job.task) }
        for feed in feeds { _ = await feed.result }
        if batch === running { batch = nil }
        publishOffline()
    }

    /// True when this caller was cancelled. The joined task keeps running.
    private func cancelledWhileJoining(_ task: Task<Void, Never>) async -> Bool {
        if Task.isCancelled { return true }
        let box = ResumeBox()
        return await withTaskCancellationHandler {
            await withCheckedContinuation { (cont: CheckedContinuation<Bool, Never>) in
                box.start(cont)
                Task {
                    await task.value
                    box.finish(false)
                }
            }
        } onCancel: {
            box.finish(true)
        }
    }
}

/// Waits for a task without cancelling it when the waiter is cancelled.
private func waitUntilDone(_ task: Task<Void, Never>) async {
    await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
        Task {
            await task.value
            cont.resume()
        }
    }
}

private func offMain<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T {
    await withCheckedContinuation { cont in
        HostedOfflineProbe.queue.async {
            HostedOfflineProbe.log.note()
            cont.resume(returning: work())
        }
    }
}

private func isCancellation(_ error: Error) -> Bool {
    if error is CancellationError { return true }
    if let url = error as? URLError, url.code == .cancelled { return true }
    return false
}

private func errorText(_ error: Error) -> String {
    if let hosted = error as? HostedException, !hosted.message.isEmpty { return hosted.message }
    let text = error.localizedDescription
    return text.isEmpty ? "Could not download." : text
}

private enum PostsFile: Sendable {
    case missing, current, mismatch
}

private struct PostsScan: Sendable {
    var gone: [String]
    var present: [String]
    var stale: Set<String>
}

private func postsFileExists(root: URL, id: String) -> Bool {
    let url = root.appendingPathComponent(id, isDirectory: true).appendingPathComponent(hostedPostsCache)
    return FileManager.default.fileExists(atPath: url.path)
}

private func inspectPosts(root: URL, id: String, knownStale: Bool) -> PostsFile {
    let url = root.appendingPathComponent(id, isDirectory: true).appendingPathComponent(hostedPostsCache)
    guard FileManager.default.fileExists(atPath: url.path) else { return .missing }
    if knownStale { return .mismatch }
    return readHostedPosts(file: url) == nil ? .mismatch : .current
}

private func readHostedPosts(root: URL, id: String) -> FeedCache? {
    let url = root.appendingPathComponent(id, isDirectory: true).appendingPathComponent(hostedPostsCache)
    guard FileManager.default.fileExists(atPath: url.path) else { return nil }
    return readHostedPosts(file: url)
}

private func readHostedPosts(file: URL) -> FeedCache? {
    HostedOfflineProbe.log.noteDecode()
    guard let cache = JSONFiles.read(FeedCache.self, from: file), cache.version == hostedPostsCacheVersion else {
        return nil
    }
    return cache
}

private func writePostsFile(root: URL, id: String, posts: [FeedPost]) {
    let dir = root.appendingPathComponent(id, isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    try? FileManager.default.removeItem(at: dir.appendingPathComponent("book.epub"))
    try? FileManager.default.removeItem(at: dir.appendingPathComponent("feed.json"))
    try? JSONFiles.write(FeedCache(version: hostedPostsCacheVersion, posts: posts), to: dir.appendingPathComponent(hostedPostsCache))
}

private func readSocialFile(root: URL, id: String) -> SocialCache? {
    let url = root.appendingPathComponent(id, isDirectory: true).appendingPathComponent(hostedSocialFile)
    guard let cache = JSONFiles.read(SocialCache.self, from: url), cache.version == socialCacheVersion else {
        return nil
    }
    return cache
}

private let hostedSocialFile = "social.json"

/// Decode attempts of a posts file that exists, and which threads touched hosted files.
enum HostedOfflineProbe {
    static let log = HostedDiskLog()
    static let queue = DispatchQueue(label: "mewo.hosted.disk", qos: .userInitiated)
    static var beforeClaim: (@MainActor () async -> Void)?
}

final class HostedDiskLog: @unchecked Sendable {
    private let lock = NSLock()
    private var flags: [Bool] = []
    private var decodes = 0

    func note() {
        lock.lock()
        flags.append(Thread.isMainThread)
        lock.unlock()
    }

    func noteDecode() {
        lock.lock()
        decodes += 1
        lock.unlock()
    }

    func reset() {
        lock.lock()
        flags.removeAll()
        decodes = 0
        lock.unlock()
    }

    var decodeCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return decodes
    }

    var touchedMain: Bool {
        lock.lock()
        defer { lock.unlock() }
        return flags.contains(true)
    }

    var touches: Int {
        lock.lock()
        defer { lock.unlock() }
        return flags.count
    }
}

/// The session token and the shelf ids, read inside the same lock as a file write or wipe.
final class HostedFileGate: @unchecked Sendable {
    private let lock = NSLock()
    private var token: String?
    private var shelf: Set<String> = []

    func setToken(_ token: String?) {
        lock.lock()
        self.token = token
        lock.unlock()
    }

    func setShelf(_ ids: Set<String>) {
        lock.lock()
        shelf = ids
        lock.unlock()
    }

    func sync<T>(_ body: (String?, Set<String>) -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return body(token, shelf)
    }
}

private struct DownloadClaim {
    var job: DownloadJob
    var started: Bool
}

private final class DownloadJob: @unchecked Sendable {
    let task: Task<Void, Never>
    private let lock = NSLock()
    private var finished = false

    init(task: Task<Void, Never>) {
        self.task = task
    }

    func markFinished() {
        lock.lock()
        finished = true
        lock.unlock()
    }

    var isFinished: Bool {
        lock.lock()
        defer { lock.unlock() }
        return finished
    }

    func cancel() { task.cancel() }
}

/// The task exists before `job` is readable. `start` is what lets the body see the job,
/// and the caller does that only after the job is in `batch` or `bookJobs`.
private final class PreparedDownload {
    let job: DownloadJob
    private let gate: StartGate

    init(body: @escaping @MainActor (DownloadJob) async -> Void) {
        let gate = StartGate()
        let box = DownloadBox()
        let task = Task { @MainActor in
            await gate.wait()
            await body(box.job)
        }
        let job = DownloadJob(task: task)
        box.job = job
        self.gate = gate
        self.job = job
    }

    func start() { gate.release() }
}

private final class DownloadBox {
    var job: DownloadJob!
}

private final class StartGate: @unchecked Sendable {
    private let lock = NSLock()
    private var waiters: [CheckedContinuation<Void, Never>] = []
    private var open = false

    func wait() async {
        await withCheckedContinuation { cont in
            lock.lock()
            if open {
                lock.unlock()
                cont.resume()
            } else {
                waiters.append(cont)
                lock.unlock()
            }
        }
    }

    func release() {
        lock.lock()
        open = true
        let pending = waiters
        waiters.removeAll()
        lock.unlock()
        pending.forEach { $0.resume() }
    }
}

private final class ResumeBox: @unchecked Sendable {
    private let lock = NSLock()
    private var cont: CheckedContinuation<Bool, Never>?
    private var value: Bool?

    func start(_ cont: CheckedContinuation<Bool, Never>) {
        lock.lock()
        if let value {
            lock.unlock()
            cont.resume(returning: value)
            return
        }
        self.cont = cont
        lock.unlock()
    }

    func finish(_ cancelled: Bool) {
        lock.lock()
        if value != nil {
            lock.unlock()
            return
        }
        value = cancelled
        let cont = self.cont
        self.cont = nil
        lock.unlock()
        cont?.resume(returning: cancelled)
    }
}

func expireOn401<T>(
    sessions: HostedSessionStore,
    session: HostedSession,
    operation: () async throws -> T
) async throws -> T {
    do {
        return try await operation()
    } catch let err as HostedException where err.code == 401 {
        if sessions.session.token == session.token {
            sessions.signOut()
        }
        throw err
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

/// Server posts cached per hosted book. Bump the version if the server's post shape changes.
private let hostedPostsCache = "posts.json"
private let hostedPostsCacheVersion = 1

/// One library refresh, tied to the session token that started it.
private struct RefreshFlight {
    let id: UUID
    let task: Task<Void, Error>
    let token: String?
}

/// Waits for a refresh without cancelling it when the caller is cancelled.
private func wait(_ task: Task<Void, Error>) async throws {
    try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
        Task {
            cont.resume(with: await task.result)
        }
    }
}
