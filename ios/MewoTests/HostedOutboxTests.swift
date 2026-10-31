import XCTest
@testable import Mewo

@MainActor
final class HostedOutboxTests: XCTestCase {
    private var root: URL!
    private var suite: String!
    private var defaults: UserDefaults!
    private var sessions: HostedSessionStore!
    private var script: OutboxScript!
    private var waits: RetryWaits!

    override func setUp() async throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        suite = "mewo-outbox-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)
        sessions = HostedSessionStore(defaults: defaults, persistToken: false)
        sessions.setBaseUrl(server)
        sessions.setSignedIn(username: "ada", token: "token-a")
        script = OutboxScript()
        waits = RetryWaits()
    }

    override func tearDown() async throws {
        sessions?.signOut()
        defaults?.removePersistentDomain(forName: suite)
        if let root { try? FileManager.default.removeItem(at: root) }
    }

    func testALeftoverReplyReplaysOnceWithTheSameId() async throws {
        let op = reply("c1", "hello")
        try write(ops: [op])
        script.thread = [PostComment(id: "c1", username: "ada", text: "hello", createdAt: 5, mine: true)]
        let outbox = makeOutbox()
        await outbox.attach(sessions.session)
        try await waitUntil("replay did not finish") {
            script.commentCalls == ["c1"] && outbox.queue.ops.isEmpty && !fileExists()
        }
        await outbox.drain()
        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(script.commentCalls, ["c1"])
    }

    func testDrainSkipsATerminalReplyAndSendsTheNext() async throws {
        script.commentError = HostedException(message: "That post is gone.", code: 404)
        let outbox = makeOutbox()
        await outbox.enqueue(reply("a", "first"))
        try await waitUntil("refusal did not stick") {
            refused(outbox, id: "a") == "That post is gone."
        }
        script.commentError = nil
        await outbox.enqueue(reply("b", "second"))
        try await waitUntil("the next reply did not send") {
            script.attempts == ["a", "b"] && replyIds(outbox) == ["a"]
        }
        XCTAssertEqual(refused(outbox, id: "a"), "That post is gone.")
    }

    func testA400ReplyStaysAndA409DoesNotBlockTheNextOp() async throws {
        script.commentError = HostedException(message: "Too long.", code: 400)
        let outbox = makeOutbox()
        await outbox.enqueue(reply("a", "first", isPrivate: true))
        try await waitUntil("400 did not stick") { refused(outbox, id: "a") == "Too long." }
        script.commentError = HostedException(message: "Someone else used that id.", code: 409)
        await outbox.enqueue(reply("b", "second"))
        try await waitUntil("409 did not stick") { refused(outbox, id: "b") == "Someone else used that id." }
        XCTAssertEqual(script.attempts, ["a", "b"])
        guard case .reply(let kept) = outbox.queue.ops.first else {
            return XCTFail("refused reply was dropped")
        }
        XCTAssertTrue(kept.isPrivate)
        script.commentError = nil
        await outbox.retry(replyId: "a")
        try await waitUntil("retry did not send") { replyIds(outbox) == ["b"] }
        XCTAssertEqual(script.attempts, ["a", "b", "a"])
    }

    func testA401KeepsTheFileAndA403OnLikeDropsIt() async throws {
        script.likeError = HostedException(message: "nope", code: 401)
        let outbox = makeOutbox()
        await outbox.enqueue(.like(.init(bookId: book, postId: "p", liked: true)))
        try await waitUntil("401 did not sign out") { !sessions.session.signedIn }
        let kept = try readFile()
        XCTAssertEqual(kept.account.username, "ada")
        XCTAssertEqual(kept.ops.count, 1)

        sessions.setSignedIn(username: "ada", token: "token-2")
        script.likeError = HostedException(message: "missing", code: 403)
        await outbox.attach(sessions.session)
        try await waitUntil("403 did not drop the like") { outbox.queue.ops.isEmpty && !fileExists() }
    }

    func testServerErrorsDoubleTheWaitAndResetAfterSuccess() async throws {
        script.commentError = HostedException(message: "down", code: 503)
        let outbox = makeOutbox()
        await outbox.enqueue(reply("a", "first"))
        var expected = outboxRetryNanos
        for step in 0..<6 {
            try await waitUntil("backoff \(step) did not arm") { waits.delays.count >= step + 1 }
            XCTAssertEqual(waits.delays.last, expected)
            expected = min(expected &* 2, outboxRetryMaxNanos)
            waits.release()
        }
        XCTAssertEqual(waits.delays.last, outboxRetryMaxNanos)
        script.commentError = nil
        waits.release()
        try await waitUntil("success did not clear the queue") { outbox.queue.ops.isEmpty }
        script.commentError = HostedException(message: "down", code: 500)
        await outbox.enqueue(reply("b", "second"))
        try await waitUntil("backoff did not reset") { waits.delays.last == outboxRetryNanos }
    }

    func testEnqueueDuringBackoffDoesNotHitTheServer() async throws {
        script.commentError = HostedException(message: "down", code: 500)
        let outbox = makeOutbox()
        await outbox.enqueue(reply("a", "one"))
        try await waitUntil("first send did not park") { script.attempts == ["a"] && waits.parked >= 1 }
        await outbox.enqueue(reply("b", "two"))
        await outbox.enqueue(reply("c", "three"))
        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(script.attempts, ["a"])
        XCTAssertEqual(waits.delays, [outboxRetryNanos])
        script.commentError = nil
        waits.release()
        try await waitUntil("the parked replies did not send") { replyIds(outbox).isEmpty }
        XCTAssertEqual(script.attempts, ["a", "a", "b", "c"])
    }

    func testBypassSendsWithoutWaitingOutTheBackoff() async throws {
        script.commentError = HostedException(message: "down", code: 500)
        let outbox = makeOutbox()
        await outbox.enqueue(reply("a", "one"))
        try await waitUntil("send did not park") { script.attempts.count == 1 && waits.parked >= 1 }
        outbox.drain(bypassBackoff: true)
        try await waitUntil("bypass did not send") { script.attempts.count == 2 }
    }

    func testBackgroundCancelsTheRetryTimer() async throws {
        script.commentError = URLError(.notConnectedToInternet)
        let outbox = makeOutbox()
        await outbox.enqueue(reply("a", "one"))
        try await waitUntil("timer did not park") { waits.parked >= 1 }
        XCTAssertEqual(script.attempts.count, 1)
        outbox.setForeground(false)
        try await waitUntil("background did not cancel the timer") { waits.parked == 0 }
        waits.release()
        try await Task.sleep(nanoseconds: 40_000_000)
        XCTAssertEqual(script.attempts.count, 1)
        outbox.setForeground(true)
        try await waitUntil("foreground did not re-arm") { waits.parked >= 1 }
        XCTAssertEqual(script.attempts.count, 1)
    }

    func testSignOutKeepsTheFileAndAnotherAccountDeletesIt() async throws {
        script.commentError = offlineError()
        let outbox = makeOutbox()
        await outbox.enqueue(reply("c1", "offline note"))
        try await waitUntil("the reply was not saved") { fileExists() }
        XCTAssertFalse(outboxFile().path.contains("/hosted/"))
        sessions.signOut()
        await outbox.attach(sessions.session)
        XCTAssertEqual(outbox.unsentReplies, 0)
        XCTAssertTrue(fileExists())
        XCTAssertEqual(try readFile().account.username, "ada")

        sessions.setSignedIn(username: "ada", token: "token-a")
        script.commentError = nil
        script.thread = [PostComment(id: "c1", username: "ada", text: "offline note", createdAt: 5, mine: true)]
        await outbox.attach(sessions.session)
        try await waitUntil("the same account did not resume") {
            script.commentCalls == ["c1"] && outbox.queue.ops.isEmpty && !fileExists()
        }

        script.commentError = offlineError()
        await outbox.enqueue(reply("c2", "second"))
        try await waitUntil("the second reply was not saved") { fileExists() }
        sessions.setSignedIn(username: "bea", token: "token-b")
        await outbox.attach(sessions.session)
        try await waitUntil("the other account did not delete the file") { !fileExists() }
        XCTAssertEqual(outbox.unsentReplies, 0)
        XCTAssertEqual(script.commentCalls, ["c1"])
    }

    func testDiscardAllDeletesAReplyThatLandsAndSendsNothingElse() async throws {
        script.holdComments = true
        let outbox = makeOutbox()
        let sending = Task { await outbox.enqueue(reply("c1", "one")) }
        try await waitUntil("the reply did not reach the wire") { script.commentEntered >= 1 }
        await outbox.enqueue(reply("c2", "two"))
        let discarding = Task { await outbox.discardAll() }
        try await Task.sleep(nanoseconds: 40_000_000)
        script.thread = [PostComment(id: "c1", username: "ada", text: "one", createdAt: 5, mine: true)]
        script.releaseComments()
        await sending.value
        await discarding.value
        XCTAssertEqual(script.commentCalls, ["c1"])
        XCTAssertEqual(script.deleted, ["c1"])
        XCTAssertTrue(outbox.queue.ops.isEmpty)
        XCTAssertFalse(fileExists())
    }

    func testACorruptFileIsQuarantinedOnce() async throws {
        try Data("{not json".utf8).write(to: outboxFile())
        let outbox = makeOutbox()
        await outbox.attach(sessions.session)
        let bad = outboxFile().appendingPathExtension("bad")
        XCTAssertEqual(try String(contentsOf: bad, encoding: .utf8), "{not json")
        XCTAssertFalse(fileExists())
        await outbox.enqueue(reply("c1", "hi"))
        try await waitUntil("enqueue after quarantine did not send") {
            script.commentCalls == ["c1"] && outbox.queue.ops.isEmpty
        }

        try Data("{again".utf8).write(to: outboxFile())
        await outbox.attach(sessions.session)
        let names = try FileManager.default.contentsOfDirectory(atPath: root.path).filter { $0.hasSuffix(".bad") }
        XCTAssertEqual(names, [bad.lastPathComponent])
        XCTAssertEqual(try String(contentsOf: bad, encoding: .utf8), "{again")
    }

    func testAnUnknownVersionReadsAsEmpty() async throws {
        let body = #"{"version":99,"account":{"url":"https://mewo.example","username":"ada"},"ops":[]}"#
        try Data(body.utf8).write(to: outboxFile())
        let outbox = makeOutbox()
        await outbox.attach(sessions.session)
        XCTAssertTrue(outbox.queue.ops.isEmpty)
        XCTAssertEqual(script.commentCalls, [])
    }

    func testProgressThatLandsDuringARefreshSurvivesIt() async throws {
        let outbox = makeOutbox()
        let started = outbox.refreshStarted()
        await outbox.enqueue(.progress(.init(bookId: book, index: 6)))
        try await waitUntil("progress did not land") { outbox.queue.ops.isEmpty }
        outbox.refreshFinished(started: started)
        XCTAssertEqual(outbox.landedProgress[book], 6)
        let later = outbox.refreshStarted()
        outbox.refreshFinished(started: later)
        XCTAssertNil(outbox.landedProgress[book])
    }

    func testTwoTapsRecordOppositeIntents() async throws {
        script.holdLikes = true
        let outbox = makeOutbox()
        async let first = outbox.toggleLike(bookId: book, postId: "p", base: [])
        async let second = outbox.toggleLike(bookId: book, postId: "p", base: [])
        let liked = await first
        let cleared = await second
        XCTAssertEqual(liked, ["p"])
        XCTAssertEqual(cleared, [])
        guard case .like(let pending) = outbox.queue.ops.first else {
            return XCTFail("the second tap did not stay queued")
        }
        XCTAssertFalse(pending.liked)
        XCTAssertEqual(outbox.queue.ops.count, 1)
        script.releaseLikes()
        try await waitUntil("the taps did not settle") { outbox.queue.ops.isEmpty }
        XCTAssertEqual(script.likeCalls.last?.1, false)
        let calls = script.likeCalls.map { "\($0.0)=\($0.1)" }
        XCTAssertTrue(calls == ["p=false"] || calls == ["p=true", "p=false"], calls.joined(separator: ", "))
    }

    private func makeOutbox() -> HostedOutbox {
        let client = HostedClient()
        script.install(on: client)
        return HostedOutbox(
            file: outboxFile(),
            sessions: sessions,
            client: client,
            publish: { _, _, _ in },
            wait: { [waits] nanos in
                await waits?.wait(nanos)
            }
        )
    }

    private func outboxFile() -> URL {
        root.appendingPathComponent("hosted-outbox.json")
    }

    private func fileExists() -> Bool {
        FileManager.default.fileExists(atPath: outboxFile().path)
    }

    private func write(ops: [OutboxOp]) throws {
        let file = OutboxFile(account: OutboxAccount(url: server, username: "ada"), ops: ops)
        try JSONFiles.encoder.encode(file).write(to: outboxFile(), options: .atomic)
    }

    private func readFile() throws -> OutboxFile {
        let data = try Data(contentsOf: outboxFile())
        return try JSONFiles.decoder.decode(OutboxFile.self, from: data)
    }

    private func reply(_ id: String, _ text: String, isPrivate: Bool = false) -> OutboxOp {
        .reply(.init(
            bookId: book,
            postId: "p",
            id: id,
            text: text,
            isPrivate: isPrivate,
            createdAt: 5,
            failed: nil
        ))
    }

    private func replyIds(_ outbox: HostedOutbox) -> [String] {
        outbox.queue.ops.compactMap { op in
            guard case .reply(let reply) = op else { return nil }
            return reply.id
        }
    }

    private func refused(_ outbox: HostedOutbox, id: String) -> String? {
        for op in outbox.queue.ops {
            if case .reply(let reply) = op, reply.id == id { return reply.failed }
        }
        return nil
    }
}

private let server = "https://mewo.example"
private let book = "11111111-1111-1111-1111-111111111111"

private final class RetryWaits: @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [UInt64] = []
    private var slots: [WaitSlot] = []

    var delays: [UInt64] {
        lock.lock()
        defer { lock.unlock() }
        return recorded
    }

    var parked: Int {
        lock.lock()
        defer { lock.unlock() }
        return slots.filter { !$0.isDone }.count
    }

    func wait(_ nanos: UInt64) async {
        let slot = WaitSlot()
        lock.lock()
        recorded.append(nanos)
        slots.append(slot)
        lock.unlock()
        await withTaskCancellationHandler {
            await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
                slot.arm(cont)
            }
        } onCancel: {
            slot.finish()
        }
    }

    func release() {
        lock.lock()
        let pending = slots
        slots.removeAll()
        lock.unlock()
        pending.forEach { $0.finish() }
    }
}

private final class WaitSlot: @unchecked Sendable {
    private let lock = NSLock()
    private var cont: CheckedContinuation<Void, Never>?
    private var done = false

    var isDone: Bool {
        lock.lock()
        defer { lock.unlock() }
        return done
    }

    func arm(_ cont: CheckedContinuation<Void, Never>) {
        lock.lock()
        if done {
            lock.unlock()
            cont.resume()
            return
        }
        self.cont = cont
        lock.unlock()
    }

    func finish() {
        lock.lock()
        if done {
            lock.unlock()
            return
        }
        done = true
        let cont = self.cont
        self.cont = nil
        lock.unlock()
        cont?.resume()
    }
}

private final class OutboxScript: @unchecked Sendable {
    private let lock = NSLock()
    var likeError: Error?
    var commentError: Error?
    var holdComments = false
    var holdLikes = false
    var thread: [PostComment] = []
    private var likes: Set<String> = []
    private var comments: [String] = []
    private var attemptIds: [String] = []
    private var likeLog: [(String, Bool)] = []
    private var deletedIds: [String] = []
    private var commentArrivals = 0
    private let commentsGate = QuietPark()
    private let likesGate = QuietPark()

    var commentCalls: [String] {
        lock.lock(); defer { lock.unlock() }
        return comments
    }

    var attempts: [String] {
        lock.lock(); defer { lock.unlock() }
        return attemptIds
    }

    var likeCalls: [(String, Bool)] {
        lock.lock(); defer { lock.unlock() }
        return likeLog
    }

    var deleted: [String] {
        lock.lock(); defer { lock.unlock() }
        return deletedIds
    }

    var commentEntered: Int {
        lock.lock(); defer { lock.unlock() }
        return commentArrivals
    }

    func releaseComments() { commentsGate.release() }
    func releaseLikes() { likesGate.release() }

    func install(on client: HostedClient) {
        client.hooks.setLike = { [weak self] _, _, postId, on in
            guard let self else { throw CancellationError() }
            if self.holdLikes { await self.likesGate.wait() }
            if let likeError = self.likeError { throw likeError }
            return self.recordLike(postId, on: on)
        }
        client.hooks.addComment = { [weak self] _, _, _, text, commentId, _ in
            guard let self else { throw CancellationError() }
            self.noteCommentEntered()
            if self.holdComments { await self.commentsGate.wait() }
            self.noteAttempt(commentId)
            if let commentError = self.commentError { throw commentError }
            self.noteComment(commentId)
            let saved = self.thread
            if !saved.isEmpty { return saved }
            return [PostComment(id: commentId, username: "ada", text: text, createdAt: 5, mine: true)]
        }
        client.hooks.deleteComment = { [weak self] _, _, _, commentId in
            guard let self else { throw CancellationError() }
            self.noteDeleted(commentId)
            return []
        }
        client.hooks.patch = { _, id, body in
            BookRecord(
                id: id,
                title: "Emma",
                author: "Austen",
                handle: "austen",
                importedAt: 1,
                progressIndex: body.progressIndex ?? 0
            )
        }
    }

    private func noteCommentEntered() {
        lock.lock(); commentArrivals += 1; lock.unlock()
    }

    private func noteAttempt(_ id: String) {
        lock.lock(); attemptIds.append(id); lock.unlock()
    }

    private func noteComment(_ id: String) {
        lock.lock(); comments.append(id); lock.unlock()
    }

    private func noteDeleted(_ id: String) {
        lock.lock(); deletedIds.append(id); lock.unlock()
    }

    private func recordLike(_ postId: String, on: Bool) -> Set<String> {
        lock.lock()
        likeLog.append((postId, on))
        if on { likes.insert(postId) } else { likes.remove(postId) }
        let ids = likes
        lock.unlock()
        return ids
    }
}
