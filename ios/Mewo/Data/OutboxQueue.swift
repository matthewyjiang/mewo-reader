import Foundation

/// Who the queue belongs to. A queue is only drained for this exact account.
struct OutboxAccount: Codable, Equatable, Sendable {
    var url: String
    var username: String
}

extension HostedSession {
    /// Signed in, with a normalized URL and a non-blank username.
    var outboxAccount: OutboxAccount? {
        guard signedIn, let url = normalizedUrl() else { return nil }
        guard let username, !username.isEmpty else { return nil }
        return OutboxAccount(url: url, username: username)
    }
}

enum OutboxOp: Equatable, Sendable {
    case reply(Reply)
    case like(Like)
    case progress(Progress)

    struct Reply: Codable, Equatable, Sendable {
        var bookId: String
        var postId: String
        /// Lowercase UUID, the server comment id.
        var id: String
        var text: String
        var isPrivate: Bool
        var createdAt: Int64
        /// Terminal refusal, kept until Retry or Delete.
        var failed: String?
    }

    struct Like: Codable, Equatable, Sendable {
        var bookId: String
        var postId: String
        var liked: Bool
    }

    struct Progress: Codable, Equatable, Sendable {
        var bookId: String
        var index: Int
    }

    var bookId: String {
        switch self {
        case .reply(let reply): return reply.bookId
        case .like(let like): return like.bookId
        case .progress(let progress): return progress.bookId
        }
    }
}

enum OutboxOutcome: Equatable {
    case retry
    case hold401
    case refused(String)
    case gone
    case accepted
}

/// Transport and throttle errors retry. 401 holds the file. Any other 4xx is
/// terminal. A 2xx whose body did not decode is accepted.
/// Cancellation is not an outcome: `isOutboxCancellation` stops the pass with no backoff.
func classifyOutbox(_ error: Error) -> OutboxOutcome {
    if let url = error as? URLError, url.code != .cancelled {
        return .retry
    }
    guard let hosted = error as? HostedException else { return .retry }
    let message = hosted.message.isEmpty ? "Could not send." : hosted.message
    switch hosted.code {
    case 401:
        return .hold401
    case 408, 429:
        return .retry
    case 500..<600:
        return .retry
    case 200..<300:
        return .accepted
    case 403, 404:
        return .gone
    case 400..<500:
        return .refused(message)
    default:
        return .retry
    }
}

func isOutboxCancellation(_ error: Error) -> Bool {
    if error is CancellationError { return true }
    if let url = error as? URLError, url.code == .cancelled { return true }
    return false
}

/// Same formula as the comments_private trigger: the commenter is not the owner,
/// and the book is not shared and not the sample. The server ORs this in, so a
/// later share cannot publish a reply written here.
func replyIsPrivate(_ book: BookRecord) -> Bool {
    !book.mine && !book.shared && !book.isSample
}

/// Lowercase hyphenated UUID. The server rejects the uppercase form Swift prints.
func outboxReplyId() -> String {
    UUID().uuidString.lowercased()
}

let outboxRetryNanos: UInt64 = 30_000_000_000
let outboxRetryMaxNanos: UInt64 = 300_000_000_000
let outboxFileVersion = 1

/// Title for the sign-out confirm. One reply stays singular.
func discardUnsentTitle(_ count: Int) -> String {
    let noun = count == 1 ? "reply" : "replies"
    return "Discard \(count) unsent \(noun)?"
}

/// Pending likes, replies, and progress. No I/O. Likes and progress fold in
/// place, so a coalesced op keeps its original position. Replies append.
struct OutboxQueue: Equatable, Sendable {
    private(set) var ops: [OutboxOp] = []

    mutating func enqueue(_ op: OutboxOp) {
        switch op {
        case .reply:
            ops.append(op)
        case .like(let like):
            if let index = ops.firstIndex(where: { item in
                guard case .like(let pending) = item else { return false }
                return pending.bookId == like.bookId && pending.postId == like.postId
            }) {
                ops[index] = op
            } else {
                ops.append(op)
            }
        case .progress(let progress):
            if let index = ops.firstIndex(where: { item in
                guard case .progress(let pending) = item else { return false }
                return pending.bookId == progress.bookId
            }) {
                ops[index] = op
            } else {
                ops.append(op)
            }
        }
    }

    /// Intent is the opposite of the queued like, or of `base` when nothing is queued.
    mutating func flipLike(bookId: String, postId: String, base: Set<String>) {
        let pending = ops.compactMap { op -> Bool? in
            guard case .like(let like) = op, like.bookId == bookId, like.postId == postId else { return nil }
            return like.liked
        }.first
        let liked = !(pending ?? base.contains(postId))
        enqueue(.like(OutboxOp.Like(bookId: bookId, postId: postId, liked: liked)))
    }

    /// First op that is not a failed reply. A failed reply is skipped, not blocking.
    func nextSendable() -> OutboxOp? {
        ops.first { op in
            guard case .reply(let reply) = op else { return true }
            return reply.failed == nil
        }
    }

    /// Removes a reply by id. A like or progress is removed only when it is still `op`,
    /// so a newer coalesced intent survives the send of the one that was claimed.
    mutating func sent(_ op: OutboxOp) {
        switch op {
        case .reply(let reply):
            ops.removeAll { item in
                guard case .reply(let pending) = item else { return false }
                return pending.id == reply.id
            }
        case .like(let like):
            guard let index = ops.firstIndex(where: { item in
                guard case .like(let pending) = item else { return false }
                return pending.bookId == like.bookId && pending.postId == like.postId
            }), ops[index] == op else { return }
            ops.remove(at: index)
        case .progress(let progress):
            guard let index = ops.firstIndex(where: { item in
                guard case .progress(let pending) = item else { return false }
                return pending.bookId == progress.bookId
            }), ops[index] == op else { return }
            ops.remove(at: index)
        }
    }

    /// A reply stays as Not sent. A like or progress is dropped, and only when it is still `op`.
    mutating func refuse(_ op: OutboxOp, message: String) {
        switch op {
        case .reply(let reply):
            ops = ops.map { item in
                guard case .reply(var pending) = item, pending.id == reply.id else { return item }
                pending.failed = message
                return .reply(pending)
            }
        case .like, .progress:
            sent(op)
        }
    }

    mutating func retry(replyId: String) {
        ops = ops.map { item in
            guard case .reply(var reply) = item, reply.id == replyId else { return item }
            reply.failed = nil
            return .reply(reply)
        }
    }

    mutating func remove(replyId: String) {
        ops.removeAll { item in
            guard case .reply(let reply) = item else { return false }
            return reply.id == replyId
        }
    }

    /// The book left the shelf. Nothing queued for it can be sent or retried.
    mutating func dropBooks(where shouldDrop: (String) -> Bool) {
        ops.removeAll { shouldDrop($0.bookId) }
    }

    func likes(over base: Set<String>, bookId: String) -> Set<String> {
        var next = base
        for op in ops {
            guard case .like(let like) = op, like.bookId == bookId else { continue }
            if like.liked {
                next.insert(like.postId)
            } else {
                next.remove(like.postId)
            }
        }
        return next
    }

    func thread(over base: [PostComment], bookId: String, postId: String, me: String) -> [PostComment] {
        let pending = ops.compactMap { op -> OutboxOp.Reply? in
            guard case .reply(let reply) = op, reply.bookId == bookId, reply.postId == postId else { return nil }
            return reply
        }
        if pending.isEmpty { return base }
        let ids = Set(pending.map(\.id))
        let kept = base.filter { !ids.contains($0.id) }
        let extra = pending.map { reply in
            PostComment(
                id: reply.id,
                username: me,
                text: reply.text,
                createdAt: reply.createdAt,
                mine: true,
                status: reply.failed.map { .failed($0) } ?? .sending
            )
        }
        return kept + extra
    }

    /// Queued replies add one to the count unless that id is already in the cached thread, and mark the post mine.
    func index(over base: CommentIndex, bookId: String, threads: [String: [PostComment]]) -> CommentIndex {
        let replies = ops.compactMap { op -> OutboxOp.Reply? in
            guard case .reply(let reply) = op, reply.bookId == bookId else { return nil }
            return reply
        }
        if replies.isEmpty { return base }
        var counts = base.counts
        var mine = base.mine
        for reply in replies {
            let already = threads[reply.postId]?.contains { $0.id == reply.id } ?? false
            if !already {
                counts[reply.postId] = (counts[reply.postId] ?? 0) + 1
            }
            mine.insert(reply.postId)
        }
        return CommentIndex(counts: counts, mine: mine, reposts: base.reposts, reposted: base.reposted)
    }

    /// Queued progress wins. `landed` covers a place that landed while a shelf read, started earlier, is still in flight.
    func books(over rows: [BookRecord], landed: [String: Int]) -> [BookRecord] {
        var pending: [String: Int] = [:]
        for op in ops {
            guard case .progress(let progress) = op else { continue }
            pending[progress.bookId] = progress.index
        }
        if pending.isEmpty && landed.isEmpty { return rows }
        return rows.map { book in
            guard let index = pending[book.id] ?? landed[book.id], book.progressIndex != index else { return book }
            var copy = book
            copy.progressIndex = index
            return copy
        }
    }
}

extension OutboxOp: Codable {
    private enum CodingKeys: String, CodingKey {
        case type, bookId, postId, id, text, liked, index, createdAt, failed
        case isPrivate = "private"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(String.self, forKey: .type) {
        case "reply":
            self = .reply(Reply(
                bookId: try container.decode(String.self, forKey: .bookId),
                postId: try container.decode(String.self, forKey: .postId),
                id: try container.decode(String.self, forKey: .id),
                text: try container.decode(String.self, forKey: .text),
                isPrivate: try container.decodeIfPresent(Bool.self, forKey: .isPrivate) ?? false,
                createdAt: try container.decode(Int64.self, forKey: .createdAt),
                failed: try container.decodeIfPresent(String.self, forKey: .failed)
            ))
        case "like":
            self = .like(Like(
                bookId: try container.decode(String.self, forKey: .bookId),
                postId: try container.decode(String.self, forKey: .postId),
                liked: try container.decode(Bool.self, forKey: .liked)
            ))
        case "progress":
            self = .progress(Progress(
                bookId: try container.decode(String.self, forKey: .bookId),
                index: try container.decode(Int.self, forKey: .index)
            ))
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .type,
                in: container,
                debugDescription: "Unknown outbox op."
            )
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .reply(let reply):
            try container.encode("reply", forKey: .type)
            try container.encode(reply.bookId, forKey: .bookId)
            try container.encode(reply.postId, forKey: .postId)
            try container.encode(reply.id, forKey: .id)
            try container.encode(reply.text, forKey: .text)
            try container.encode(reply.isPrivate, forKey: .isPrivate)
            try container.encode(reply.createdAt, forKey: .createdAt)
            try container.encodeIfPresent(reply.failed, forKey: .failed)
        case .like(let like):
            try container.encode("like", forKey: .type)
            try container.encode(like.bookId, forKey: .bookId)
            try container.encode(like.postId, forKey: .postId)
            try container.encode(like.liked, forKey: .liked)
        case .progress(let progress):
            try container.encode("progress", forKey: .type)
            try container.encode(progress.bookId, forKey: .bookId)
            try container.encode(progress.index, forKey: .index)
        }
    }
}

struct OutboxFile: Codable, Equatable, Sendable {
    var version: Int = outboxFileVersion
    var account: OutboxAccount
    var ops: [OutboxOp] = []
}
