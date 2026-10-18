import XCTest
@testable import Mewo

@MainActor
final class HostedLibraryRefreshTests: XCTestCase {
    private var root: URL!
    private var suite: String!
    private var defaults: UserDefaults!
    private var sessions: HostedSessionStore!
    private var client: GateClient!
    private var store: HostedLibraryStore!

    override func setUp() async throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        suite = "mewo-shelf-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)
        sessions = HostedSessionStore(defaults: defaults, persistToken: false)
        sessions.setBaseUrl(server)
        client = GateClient()
        store = HostedLibraryStore(opener: EpubOpener(), sessions: sessions, client: client.client, files: root)
    }

    override func tearDown() async throws {
        client.failAll()
        sessions.signOut()
        store = nil
        defaults.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: root)
    }

    func testSwitchingAccountsDoesNotLeakTheShelfOrSignOut() async throws {
        let ada = book(privateId, title: "Ada's diary", owner: "ada")
        try shelfCache().write(serverUrl: server, username: "ada", books: [ada])
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await client.waitForCalls(1)
        XCTAssertEqual(store.books.map(\.id), [privateId])

        client.expireToken = "token-a"
        sessions.setSignedIn(username: "bea", token: "token-b")
        let whileAIsInFlight = store.books.map(\.id)
        client.respond("token-a", books: [ada])
        try await Task.sleep(nanoseconds: 200_000_000)
        XCTAssertFalse(
            whileAIsInFlight.contains(privateId) || sessions.session.token != "token-b",
            "B saw \(whileAIsInFlight) token=\(sessions.session.token ?? "nil")"
        )
        try await client.waitForCalls(2)
        client.respond("token-b", books: [book(beaId, title: "Bea's book", owner: "bea")])
        try await waitUntil("Bea's book did not replace the shelf") {
            store.books.contains { $0.id == beaId }
        }
        XCTAssertEqual(sessions.session.token, "token-b")
        XCTAssertEqual(sessions.session.username, "bea")
        XCTAssertEqual(store.books.map(\.id), [beaId])
        XCTAssertFalse(store.books.contains { $0.id == privateId })
    }

    func testOpeningABookDuringRefreshDoesNotKeepRowsTheServerRemoved() async throws {
        let stale = book(staleId, title: "Removed", postCount: 12)
        let kept = book(keptId, title: "Kept", postCount: 4)
        try shelfCache().write(serverUrl: server, username: "ada", books: [stale, kept])
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await client.waitForCalls(1)
        Task { try? await store.feed(id: staleId) }
        try await waitUntil("opening the cached book did not touch the row") {
            store.books.first { $0.id == staleId }?.postCount == 1
        }
        client.respond("token-a", books: [kept])
        try await waitUntil("refresh did not finish") { !store.loadState.isRefreshing }
        XCTAssertEqual(store.books.map(\.id), [keptId])
        let posts = root.appendingPathComponent("hosted/\(staleId)/posts.json")
        XCTAssertFalse(FileManager.default.fileExists(atPath: posts.path))
        try JSONFiles.write(
            FeedCache(version: 1, posts: [FeedPost(id: "p", kind: .body, text: "secret", chapter: "1")]),
            to: posts
        )
        do {
            _ = try await store.feed(id: staleId)
            XCTFail("opened a book the refresh removed")
        } catch {
            XCTAssertEqual(error.localizedDescription, "That book is not on your shelf.")
        }
    }

    func testAFullDiskDoesNotFailASuccessfulRefresh() async throws {
        let shelves = root.appendingPathComponent("hosted-shelves")
        try Data("not a directory".utf8).write(to: shelves)
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await client.waitForCalls(1)
        client.respond("token-a", books: [book(keptId, title: "Kept")])
        try await waitUntil("shelf did not settle") {
            if case .loaded = store.loadState { return !store.loadState.isRefreshing }
            return false
        }
        guard case .loaded = store.loadState else {
            return XCTFail("load state \(store.loadState)")
        }
        XCTAssertEqual(store.books.map(\.id), [keptId])
    }

    func testAnExpiredTokenSignsThatAccountOut() async throws {
        client.expireToken = "token-a"
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await client.waitForCalls(1)
        client.respond("token-a", books: [book(privateId, title: "Ada's diary")])
        try await waitUntil("expired token stayed signed in") { sessions.session.token == nil }
        XCTAssertFalse(store.books.contains { $0.id == privateId })
    }

    func testASecondPullJoinsTheInFlightRefresh() async throws {
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await client.waitForCalls(1)
        async let joined: Void = store.load()
        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(client.libraryCalls, 1)
        client.respond("token-a", books: [book(keptId, title: "Kept")])
        try await joined
        let again = Task { try? await store.load() }
        try await client.waitForCalls(2)
        client.respond("token-a", books: [book(keptId, title: "Kept")])
        await again.value
        XCTAssertEqual(client.libraryCalls, 2)
        XCTAssertEqual(store.books.map(\.id), [keptId])
    }

    private func shelfCache() -> ShelfCache {
        ShelfCache(directory: root.appendingPathComponent("hosted-shelves", isDirectory: true))
    }

    private func waitUntil(_ message: String, _ condition: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(5)
        while !condition() {
            if Date() > deadline {
                XCTFail(message)
                return
            }
            try await Task.sleep(nanoseconds: 20_000_000)
        }
    }

    private func book(
        _ id: String,
        title: String,
        owner: String = "ada",
        postCount: Int = 1
    ) -> BookRecord {
        BookRecord(
            id: id,
            title: title,
            author: "Austen",
            handle: "austen",
            importedAt: 1,
            postCount: postCount,
            mine: true,
            shared: false,
            ownerUsername: owner
        )
    }
}

private final class GateClient: @unchecked Sendable {
    let client = HostedClient()
    var expireToken: String?
    private(set) var libraryCalls = 0
    private let lock = NSLock()
    private var gates: [String: UnsafeContinuation<LibrarySnapshot, Error>] = [:]
    private var startWaiters: [StartWait] = []

    init() {
        client.hooks.library = { [weak self] session in
            guard let self else { throw CancellationError() }
            return try await self.library(session)
        }
        client.hooks.posts = { _, _ in
            [FeedPost(id: "p", kind: .body, text: "secret", chapter: "1")]
        }
        client.hooks.downloadCover = { [weak self] session, _, _ in
            if session.token != nil && session.token == self?.expireToken {
                throw HostedException(message: "expired", code: 401)
            }
            return false
        }
    }

    func waitForCalls(_ count: Int) async throws {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            lock.lock()
            if libraryCalls >= count {
                lock.unlock()
                cont.resume()
                return
            }
            startWaiters.append(StartWait(count: count, cont: cont))
            lock.unlock()
        }
    }

    func respond(_ token: String, books: [BookRecord]) {
        lock.lock()
        let cont = gates.removeValue(forKey: token)
        lock.unlock()
        cont?.resume(returning: LibrarySnapshot(books: books))
    }

    func failAll() {
        lock.lock()
        let pending = gates
        gates.removeAll()
        let starts = startWaiters
        startWaiters.removeAll()
        lock.unlock()
        for cont in pending.values {
            cont.resume(throwing: CancellationError())
        }
        for start in starts {
            start.cont.resume(throwing: CancellationError())
        }
    }

    private func library(_ hosted: HostedSession) async throws -> LibrarySnapshot {
        let token = hosted.token ?? ""
        // Unsafe so a cancelled refresh still accepts the response. A 401 from that
        // response has to be delivered, not dropped, so the store can ignore it.
        return try await withUnsafeThrowingContinuation { cont in
            lock.lock()
            libraryCalls += 1
            gates[token] = cont
            let ready = startWaiters.filter { libraryCalls >= $0.count }
            startWaiters.removeAll { libraryCalls >= $0.count }
            lock.unlock()
            for start in ready {
                start.cont.resume()
            }
        }
    }

    private struct StartWait {
        var count: Int
        var cont: CheckedContinuation<Void, Error>
    }
}

private let server = "https://mewo.example"
private let privateId = "11111111-1111-1111-1111-111111111111"
private let beaId = "22222222-2222-2222-2222-222222222222"
private let staleId = "33333333-3333-3333-3333-333333333333"
private let keptId = "44444444-4444-4444-4444-444444444444"
