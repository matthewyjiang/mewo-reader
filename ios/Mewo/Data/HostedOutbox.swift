import Foundation
import os

private let outboxLog = Logger(subsystem: "com.mewo.reader", category: "outbox")

/// The only caller of hosted like, reply, and progress writes. The file lives
/// next to hosted/, so a sign-out wipe of the cache leaves the queue in place.
/// One serial queue writes the file off the main actor, in order.
@MainActor
@Observable
final class HostedOutbox {
    typealias Publish = (_ session: HostedSession, _ bookId: String, _ change: Landed) async -> Void

    enum Landed {
        case thread(postId: String, [PostComment])
        case likes(Set<String>)
    }

    private(set) var queue = OutboxQueue()
    /// Bumps when a reply appears, changes, or leaves. Likes and progress do not.
    private(set) var revision = 0

    var unsentReplies: Int {
        queue.ops.reduce(0) { count, op in
            guard case .reply = op else { return count }
            return count + 1
        }
    }

    var landedProgress: [String: Int] {
        marks.mapValues(\.index)
    }

    private struct Mark: Equatable {
        var index: Int
        var epoch: Int
    }

    private struct Flight {
        var account: OutboxAccount
        var reply: OutboxOp.Reply
        var sent: Bool
    }

    private enum Step {
        case continueDrain
        case stop
        case backoff(UInt64)
    }

    private enum SendResult {
        case empty
        case thread([PostComment])
        case likes(Set<String>)
    }

    private let file: URL
    private let sessions: HostedSessionStore
    private let client: HostedClient
    private let publish: Publish
    private let wait: (UInt64) async -> Void
    private let now: () -> Date
    private let io = DispatchQueue(label: "mewo.hosted.outbox", qos: .utility)

    private var marks: [String: Mark] = [:]
    private var refreshEpoch = 0
    /// Account `queue` was loaded for. A different account must not reuse this list.
    private var loadedFor: OutboxAccount?
    private var epoch = 0
    private var running = false
    private var queued = false
    private var retryNanos = outboxRetryNanos
    private var backoffUntil: Date?
    private var foreground = true
    private var retryTask: Task<Void, Never>?
    private var activeSignal: OutboxSignal?
    private var activeReplyId: String?
    private var awaitingDiscard: Set<String> = []
    private var flights: [String: Flight] = [:]
    private var dirty = false
    private var discardingAll = false

    init(
        file: URL,
        sessions: HostedSessionStore,
        client: HostedClient,
        publish: @escaping Publish,
        wait: @escaping (UInt64) async -> Void = { try? await Task.sleep(nanoseconds: $0) },
        now: @escaping () -> Date = Date.init
    ) {
        self.file = file
        self.sessions = sessions
        self.client = client
        self.publish = publish
        self.wait = wait
        self.now = now
    }

    /// Memory first, then the file, then a drain. A disk error is logged and the op stays.
    /// A signed-in session with no username has no queue, so the call fails instead of dropping the op.
    func enqueue(_ op: OutboxOp) async throws {
        guard !discardingAll else { return }
        guard let acct = account() else { throw notSignedIn() }
        if await prepare(acct) == false { return }
        guard !discardingAll, account() == acct else { return }
        var next = queue
        next.enqueue(op)
        commit(next)
        await persist()
        drain()
    }

    /// Flips against the pending like, over the cached base. The flip happens
    /// before this method's first await once the queue is already loaded, so two
    /// taps cannot both record the same intent.
    func toggleLike(bookId: String, postId: String, base: Set<String>) async throws -> Set<String> {
        guard !discardingAll else { return base }
        guard let acct = account() else { throw notSignedIn() }
        if loadedFor != acct {
            if await prepare(acct) == false { return base }
        }
        guard !discardingAll, account() == acct else { return base }
        var next = queue
        next.flipLike(bookId: bookId, postId: postId, base: base)
        let shown = next.likes(over: base, bookId: bookId)
        commit(next)
        await persist()
        drain()
        return shown
    }

    /// A call during a run sets `queued` and the running pass starts another when it finishes.
    func drain(bypassBackoff: Bool = false) {
        if bypassBackoff {
            backoffUntil = nil
            retryTask?.cancel()
            retryTask = nil
        }
        if running {
            queued = true
            return
        }
        running = true
        queued = false
        Task { @MainActor [weak self] in
            guard let self else { return }
            await self.drainPass()
            await self.finishDrain()
        }
    }

    /// Off cancels the retry timer. On drains if the backoff already elapsed, otherwise re-arms it.
    func setForeground(_ on: Bool) {
        foreground = on
        if !on {
            retryTask?.cancel()
            retryTask = nil
            return
        }
        guard let until = backoffUntil else { return }
        let current = now()
        if until <= current {
            backoffUntil = nil
            drain(bypassBackoff: true)
        } else {
            let remain = UInt64(until.timeIntervalSince(current) * 1_000_000_000)
            armRetry(remain)
        }
    }

    func refreshStarted() -> Int {
        refreshEpoch += 1
        return refreshEpoch
    }

    /// Places confirmed before `started` are dropped. A fetch that was already
    /// running when the place landed keeps the overlay.
    func refreshFinished(started: Int) {
        let next = marks.filter { _, mark in started <= mark.epoch }
        if next.count != marks.count {
            marks = next
        }
    }

    func retry(replyId: String) async {
        guard let acct = account() else { return }
        if await prepare(acct) == false { return }
        var next = queue
        next.retry(replyId: replyId)
        guard next != queue else { return }
        commit(next)
        await persist()
        drain(bypassBackoff: true)
    }

    /// A request already on the wire is waited out. If that request landed, the server row is deleted.
    func discard(replyId: String) async {
        let signal: OutboxSignal?
        if activeReplyId == replyId, let active = activeSignal {
            awaitingDiscard.insert(replyId)
            signal = active
        } else {
            var next = queue
            next.remove(replyId: replyId)
            if next != queue {
                commit(next)
                await persist()
            }
            signal = nil
        }
        guard let signal else { return }
        await signal.wait()
        awaitingDiscard.remove(replyId)
        let flight = flights.removeValue(forKey: replyId)
        var next = queue
        next.remove(replyId: replyId)
        if next != queue {
            commit(next)
            await persist()
        }
        if let flight, flight.sent {
            await deleteSent(flight)
        }
    }

    /// Explicit sign-out. The queue is cleared before the in-flight reply returns,
    /// so the rest of the file cannot send. A reply that lands is deleted.
    func discardAll() async {
        discardingAll = true
        defer { discardingAll = false }
        epoch += 1
        queued = false
        backoffUntil = nil
        retryTask?.cancel()
        retryTask = nil
        let id = activeReplyId
        if let id { awaitingDiscard.insert(id) }
        let signal = id == nil ? nil : activeSignal
        loadedFor = account()
        commit(OutboxQueue(), progress: [:])
        await persist()
        await deleteBad()
        guard let signal, let id else { return }
        await signal.wait()
        awaitingDiscard.remove(id)
        let flight = flights.removeValue(forKey: id)
        if let flight, flight.sent {
            await deleteSent(flight)
        }
    }

    /// Drops every op for a book that was deleted or left the shelf.
    func drop(bookId: String) async {
        await dropBooks { $0 == bookId }
    }

    /// Drops ops whose book is not in the shelf a refresh just published.
    func dropExcept(_ kept: Set<String>) async {
        await dropBooks { !kept.contains($0) }
    }

    /// Signed out keeps the file and clears memory. A different account deletes it.
    /// The same account reloads it and drains.
    func attach(_ session: HostedSession) async {
        guard sessions.session.outboxAccount == session.outboxAccount else { return }
        let stored = await readStored()
        guard sessions.session.outboxAccount == session.outboxAccount else { return }
        let next = session.outboxAccount
        let resume: Bool
        switch stored {
        case .corrupt:
            await quarantine()
        case .empty, .file:
            break
        }
        let fileAccount = stored.account
        let fileOps = stored.ops
        if next == nil {
            epoch += 1
            queued = false
            backoffUntil = nil
            retryTask?.cancel()
            retryTask = nil
            loadedFor = nil
            commit(OutboxQueue(), progress: [:])
            await deleteBad()
            resume = false
        } else if let fileAccount, fileAccount != next {
            epoch += 1
            queued = false
            backoffUntil = nil
            retryTask?.cancel()
            retryTask = nil
            loadedFor = next
            commit(OutboxQueue(), progress: [:])
            await persist()
            await deleteBad()
            resume = false
        } else if let loadedFor, loadedFor != next {
            epoch += 1
            queued = false
            backoffUntil = nil
            retryTask?.cancel()
            retryTask = nil
            self.loadedFor = next
            var nextQueue = OutboxQueue()
            for op in fileOps { nextQueue.enqueue(op) }
            commit(nextQueue, progress: [:])
            await deleteBad()
            resume = true
        } else {
            loadedFor = next
            var nextQueue = OutboxQueue()
            for op in fileOps { nextQueue.enqueue(op) }
            commit(nextQueue)
            resume = !fileOps.isEmpty
        }
        if resume { drain() }
    }

    /// Loads the account's file when memory is for someone else. False means the
    /// file belongs to another account and must be left alone.
    private func prepare(_ acct: OutboxAccount) async -> Bool {
        if loadedFor == acct { return true }
        let stored = await readStored()
        guard account() == acct else { return false }
        if loadedFor == acct { return true }
        switch stored {
        case .corrupt:
            await quarantine()
            loadedFor = acct
            commit(OutboxQueue(), progress: [:])
            return true
        case .empty:
            loadedFor = acct
            commit(OutboxQueue(), progress: [:])
            return true
        case .file(let snapshot):
            if snapshot.account != acct {
                epoch += 1
                queued = false
                backoffUntil = nil
                retryTask?.cancel()
                retryTask = nil
                loadedFor = acct
                commit(OutboxQueue(), progress: [:])
                await persist()
                await deleteBad()
                return true
            }
            loadedFor = acct
            var nextQueue = OutboxQueue()
            for op in snapshot.ops { nextQueue.enqueue(op) }
            commit(nextQueue)
            return true
        }
    }

    private func drainPass() async {
        guard let acct = account() else { return }
        let passEpoch = epoch
        if let until = backoffUntil, until > now() { return }
        if loadedFor != acct {
            guard await prepare(acct), epoch == passEpoch, account() == acct else { return }
        }
        if dirty { await persist() }
        while true {
            if epoch != passEpoch || account() != acct { return }
            if let until = backoffUntil, until > now() { return }
            guard let op = queue.nextSendable() else { return }
            let signal = OutboxSignal()
            activeSignal = signal
            activeReplyId = op.replyId
            var failure: Error?
            var sent: SendResult?
            var skipped = false
            let session = sessions.session
            if session.outboxAccount != acct {
                skipped = true
            } else {
                do {
                    sent = try await send(session, op)
                } catch {
                    failure = error
                }
            }
            let step: Step
            if let failure, isOutboxCancellation(failure) {
                step = .stop
            } else if epoch != passEpoch || account() != acct || skipped {
                rememberLanded(op, sent: sent, acct: acct)
                step = .stop
            } else if let failure {
                step = await applyFailure(op, failure)
            } else {
                await applySuccess(acct, session, op, sent)
                step = .continueDrain
            }
            if activeSignal === signal {
                activeSignal = nil
                activeReplyId = nil
            }
            signal.finish()
            switch step {
            case .continueDrain:
                continue
            case .stop:
                return
            case .backoff(let wait):
                if epoch == passEpoch && account() == acct {
                    armRetry(wait)
                }
                return
            }
        }
    }

    private func finishDrain() async {
        while queued {
            queued = false
            await drainPass()
        }
        running = false
    }

    /// The cache write finishes before the op leaves the queue. A discard that
    /// starts during either await still deletes a reply that already landed.
    private func applySuccess(_ acct: OutboxAccount, _ session: HostedSession, _ op: OutboxOp, _ sent: SendResult?) async {
        let pass = epoch
        if case .reply(let reply) = op, case .thread(let comments) = sent {
            if awaitingDiscard.contains(reply.id) {
                rememberLanded(op, sent: sent, acct: acct)
            } else if sessions.session.outboxAccount == acct {
                await publish(session, reply.bookId, .thread(postId: reply.postId, comments))
            }
        }
        if case .like = op, case .likes(let ids) = sent, sessions.session.outboxAccount == acct {
            await publish(session, op.bookId, .likes(ids))
        }
        if epoch != pass || account() != acct {
            rememberLanded(op, sent: sent, acct: acct)
            return
        }
        var progress = marks
        if case .progress(let mark) = op {
            progress[mark.bookId] = Mark(index: mark.index, epoch: refreshEpoch)
        }
        var next = queue
        next.sent(op)
        if next != queue || progress != marks {
            commit(next, progress: progress)
            await persist()
        }
        if discarded(since: pass, op, acct) {
            rememberLanded(op, sent: sent, acct: acct)
            return
        }
        retryNanos = outboxRetryNanos
        backoffUntil = nil
        retryTask?.cancel()
        retryTask = nil
    }

    /// True when sign-out replaced the queue, or this reply was discarded, during the write.
    private func discarded(since pass: Int, _ op: OutboxOp, _ acct: OutboxAccount) -> Bool {
        if epoch != pass || account() != acct { return true }
        guard case .reply(let reply) = op else { return false }
        return awaitingDiscard.contains(reply.id)
    }

    private func applyFailure(_ op: OutboxOp, _ error: Error) async -> Step {
        switch classifyOutbox(error) {
        case .retry:
            let wait = takeBackoff()
            backoffUntil = now().addingTimeInterval(Double(wait) / 1_000_000_000)
            return .backoff(wait)
        case .hold401:
            return .stop
        case .accepted:
            await drop(op)
            return .continueDrain
        case .refused(let message):
            await refuse(op, message)
            return .continueDrain
        case .gone:
            let message = (error as? HostedException)?.message
            let text = (message?.isEmpty == false ? message : nil) ?? "Could not send."
            await refuse(op, text)
            return .continueDrain
        }
    }

    private func refuse(_ op: OutboxOp, _ message: String) async {
        var next = queue
        next.refuse(op, message: message)
        guard next != queue else { return }
        commit(next)
        await persist()
    }

    private func drop(_ op: OutboxOp) async {
        var next = queue
        next.sent(op)
        guard next != queue else { return }
        commit(next)
        await persist()
    }

    private func rememberLanded(_ op: OutboxOp, sent: SendResult?, acct: OutboxAccount) {
        guard case .reply(let reply) = op, case .thread = sent, awaitingDiscard.contains(reply.id) else { return }
        flights[reply.id] = Flight(account: acct, reply: reply, sent: true)
    }

    private func deleteSent(_ flight: Flight) async {
        let session = sessions.session
        guard session.outboxAccount == flight.account else { return }
        do {
            let thread = try await expireOn401(session) {
                try await client.deleteComment(
                    session,
                    id: flight.reply.bookId,
                    postId: flight.reply.postId,
                    commentId: flight.reply.id
                )
            }
            guard sessions.session.outboxAccount == flight.account else { return }
            await publish(session, flight.reply.bookId, .thread(postId: flight.reply.postId, thread))
        } catch {
            outboxLog.error("Could not delete a reply that landed after Discard.")
        }
    }

    private func takeBackoff() -> UInt64 {
        let wait = retryNanos
        retryNanos = min(retryNanos &* 2, outboxRetryMaxNanos)
        return wait
    }

    private func armRetry(_ wait: UInt64) {
        retryTask?.cancel()
        retryTask = nil
        guard foreground, wait > 0 else { return }
        retryTask = Task { @MainActor [weak self] in
            guard let self else { return }
            await self.wait(wait)
            guard !Task.isCancelled else { return }
            self.retryTask = nil
            self.drain(bypassBackoff: true)
        }
    }

    private func send(_ session: HostedSession, _ op: OutboxOp) async throws -> SendResult {
        switch op {
        case .like(let like):
            let ids = try await expireOn401(session) {
                try await client.setLike(session, id: like.bookId, postId: like.postId, on: like.liked)
            }
            return .likes(ids)
        case .progress(let progress):
            _ = try await expireOn401(session) {
                try await client.patch(session, id: progress.bookId, body: PatchBook(progressIndex: progress.index))
            }
            return .empty
        case .reply(let reply):
            let thread = try await expireOn401(session) {
                try await client.addComment(
                    session,
                    id: reply.bookId,
                    postId: reply.postId,
                    text: reply.text,
                    commentId: reply.id,
                    isPrivate: reply.isPrivate
                )
            }
            return .thread(thread)
        }
    }

    private func expireOn401<T>(_ session: HostedSession, _ block: () async throws -> T) async throws -> T {
        try await Mewo.expireOn401(sessions: sessions, session: session, operation: block)
    }

    private func account() -> OutboxAccount? {
        sessions.session.outboxAccount
    }

    private func notSignedIn() -> NSError {
        NSError(domain: "mewo", code: 8, userInfo: [NSLocalizedDescriptionKey: hostedNotSignedIn])
    }

    /// Removes ops and landed progress for books that are gone. An in-flight send
    /// of an op that is no longer queued does not put it back.
    private func dropBooks(where gone: (String) -> Bool) async {
        guard !discardingAll, let acct = account() else { return }
        if loadedFor != acct {
            if await prepare(acct) == false { return }
        }
        guard !discardingAll, account() == acct else { return }
        var next = queue
        next.dropBooks(where: gone)
        let progress = marks.filter { !gone($0.key) }
        guard next != queue || progress != marks else { return }
        commit(next, progress: progress)
        await persist()
    }

    private func commit(_ next: OutboxQueue, progress: [String: Mark]? = nil) {
        let before = replyStamp(queue.ops)
        queue = next
        if let progress { marks = progress }
        if before != replyStamp(next.ops) {
            revision += 1
        }
    }

    private func replyStamp(_ ops: [OutboxOp]) -> [String] {
        ops.compactMap { op in
            guard case .reply(let reply) = op else { return nil }
            return "\(reply.id)\u{0}\(reply.failed ?? "")\u{0}\(reply.text)"
        }
    }

    private func persist() async {
        let snapshot: OutboxSnapshot?
        if queue.ops.isEmpty {
            snapshot = nil
        } else if let loadedFor {
            snapshot = OutboxSnapshot(account: loadedFor, ops: queue.ops)
        } else {
            dirty = true
            return
        }
        let target = file
        let failed = await withCheckedContinuation { (cont: CheckedContinuation<Bool, Never>) in
            io.async {
                cont.resume(returning: !storeOutbox(target, snapshot))
            }
        }
        if failed {
            dirty = true
            outboxLog.error("Could not update the outbox.")
        } else {
            dirty = false
        }
    }

    private func readStored() async -> OutboxRead {
        let target = file
        return await withCheckedContinuation { cont in
            io.async {
                cont.resume(returning: readOutbox(target))
            }
        }
    }

    private func quarantine() async {
        let target = file
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            io.async {
                quarantineOutbox(target)
                cont.resume()
            }
        }
    }

    private func deleteBad() async {
        let target = file
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            io.async {
                deleteOutboxQuarantine(target)
                cont.resume()
            }
        }
    }
}

private extension OutboxOp {
    var replyId: String? {
        guard case .reply(let reply) = self else { return nil }
        return reply.id
    }

    var postId: String? {
        switch self {
        case .reply(let reply): return reply.postId
        case .like(let like): return like.postId
        case .progress: return nil
        }
    }
}

/// Resumed on the main actor when the in-flight send has been classified.
@MainActor
private final class OutboxSignal {
    private var continuation: CheckedContinuation<Void, Never>?
    private var finished = false

    func wait() async {
        if finished { return }
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            if finished {
                cont.resume()
            } else {
                continuation = cont
            }
        }
    }

    func finish() {
        finished = true
        continuation?.resume()
        continuation = nil
    }
}

private struct OutboxSnapshot: Sendable {
    var account: OutboxAccount
    var ops: [OutboxOp]
}

private enum OutboxRead: Sendable {
    case empty
    case file(OutboxSnapshot)
    case corrupt

    var account: OutboxAccount? {
        guard case .file(let snapshot) = self else { return nil }
        return snapshot.account
    }

    var ops: [OutboxOp] {
        guard case .file(let snapshot) = self else { return [] }
        return snapshot.ops
    }
}

private struct OutboxVersion: Decodable {
    var version: Int
}

/// nil deletes the file. A failure leaves the previous file in place.
private func storeOutbox(_ file: URL, _ snapshot: OutboxSnapshot?) -> Bool {
    let manager = FileManager.default
    if snapshot == nil {
        if manager.fileExists(atPath: file.path) {
            do {
                try manager.removeItem(at: file)
            } catch {
                return false
            }
        }
        return true
    }
    guard let snapshot else { return true }
    let dir = file.deletingLastPathComponent()
    do {
        try manager.createDirectory(at: dir, withIntermediateDirectories: true)
        let body = OutboxFile(version: outboxFileVersion, account: snapshot.account, ops: snapshot.ops)
        try JSONFiles.encoder.encode(body).write(to: file, options: .atomic)
        return true
    } catch {
        return false
    }
}

private func readOutbox(_ file: URL) -> OutboxRead {
    guard FileManager.default.fileExists(atPath: file.path) else { return .empty }
    guard let data = try? Data(contentsOf: file) else { return .corrupt }
    guard let header = try? JSONFiles.decoder.decode(OutboxVersion.self, from: data) else { return .corrupt }
    if header.version != outboxFileVersion { return .empty }
    guard let body = try? JSONFiles.decoder.decode(OutboxFile.self, from: data) else { return .corrupt }
    return .file(OutboxSnapshot(account: body.account, ops: body.ops))
}

/// One copy. A failed rename deletes the corrupt file so the next write can proceed.
private func quarantineOutbox(_ file: URL) {
    let manager = FileManager.default
    guard manager.fileExists(atPath: file.path) else { return }
    let bad = file.appendingPathExtension("bad")
    if manager.fileExists(atPath: bad.path) {
        try? manager.removeItem(at: bad)
    }
    do {
        try manager.moveItem(at: file, to: bad)
    } catch {
        try? manager.removeItem(at: file)
        outboxLog.error("Could not move aside a corrupt outbox.")
    }
}

private func deleteOutboxQuarantine(_ file: URL) {
    let manager = FileManager.default
    let dir = file.deletingLastPathComponent()
    let stem = file.lastPathComponent
    guard let names = try? manager.contentsOfDirectory(atPath: dir.path) else { return }
    for name in names where name.hasPrefix("\(stem).") && name.hasSuffix(".bad") {
        try? manager.removeItem(at: dir.appendingPathComponent(name))
    }
}
