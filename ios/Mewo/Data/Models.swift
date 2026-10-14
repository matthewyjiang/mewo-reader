import Foundation

struct LibrarySnapshot: Codable, Equatable, Sendable {
    var books: [BookRecord] = []
}

struct BookRecord: Codable, Equatable, Identifiable, Hashable, Sendable {
    var id: String
    var title: String
    var author: String
    var handle: String
    var importedAt: Int64
    var postCount: Int = 0
    var progressIndex: Int = 0
    var isSample: Bool = false
    var mine: Bool = true
    /// Owner chose to show this book on other accounts' shelves.
    var shared: Bool = false
    /// Username of the account that added the book. Nil on the local shelf.
    var ownerUsername: String? = nil

    enum CodingKeys: String, CodingKey {
        case id, title, author, handle, importedAt, postCount, progressIndex, isSample, mine, shared, ownerUsername
    }

    /// Saved place is the last post. Index 0 is the start, so an unopened
    /// book stays on the timeline. A one-post book finishes when the reader
    /// scrolls onto the end card and the index moves past 0.
    var isFinished: Bool {
        postCount > 0 && progressIndex >= max(1, postCount - 1)
    }
}

extension BookRecord {
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        title = try container.decode(String.self, forKey: .title)
        author = try container.decode(String.self, forKey: .author)
        handle = try container.decode(String.self, forKey: .handle)
        importedAt = try container.decode(Int64.self, forKey: .importedAt)
        postCount = try container.decodeIfPresent(Int.self, forKey: .postCount) ?? 0
        progressIndex = try container.decodeIfPresent(Int.self, forKey: .progressIndex) ?? 0
        isSample = try container.decodeIfPresent(Bool.self, forKey: .isSample) ?? false
        mine = try container.decodeIfPresent(Bool.self, forKey: .mine) ?? true
        shared = try container.decodeIfPresent(Bool.self, forKey: .shared) ?? false
        ownerUsername = try container.decodeIfPresent(String.self, forKey: .ownerUsername)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(title, forKey: .title)
        try container.encode(author, forKey: .author)
        try container.encode(handle, forKey: .handle)
        try container.encode(importedAt, forKey: .importedAt)
        try container.encode(postCount, forKey: .postCount)
        try container.encode(progressIndex, forKey: .progressIndex)
        try container.encode(isSample, forKey: .isSample)
        try container.encode(mine, forKey: .mine)
        try container.encode(shared, forKey: .shared)
        try container.encodeIfPresent(ownerUsername, forKey: .ownerUsername)
    }
}

struct FeedCache: Codable, Equatable, Sendable {
    var version: Int = 1
    var posts: [FeedPost] = []
}

struct FeedPost: Codable, Equatable, Identifiable, Hashable, Sendable {
    var id: String
    var kind: PostKind
    var text: String
    var chapter: String
}

enum PostKind: String, Codable, Sendable {
    case heading = "Heading"
    case body = "Body"
}

struct LikeSet: Codable, Equatable, Sendable {
    var ids: Set<String> = []
}

struct CommentIndex: Codable, Equatable, Sendable {
    var counts: [String: Int] = [:]
    var mine: Set<String> = []

    func count(_ postId: String) -> Int {
        counts[postId] ?? 0
    }

    func commented(_ postId: String) -> Bool {
        mine.contains(postId)
    }

    func afterThread(postId: String, comments: [PostComment]) -> CommentIndex {
        var nextCounts = counts
        if comments.isEmpty {
            nextCounts.removeValue(forKey: postId)
        } else {
            nextCounts[postId] = comments.count
        }
        var nextMine = mine
        if comments.contains(where: { $0.mine }) {
            nextMine.insert(postId)
        } else {
            nextMine.remove(postId)
        }
        return CommentIndex(counts: nextCounts, mine: nextMine)
    }
}

struct PostComment: Codable, Equatable, Hashable, Identifiable, Sendable {
    var id: String
    var username: String
    var text: String
    var createdAt: Int64
    var mine: Bool = false
}

struct CommentThread: Codable, Equatable, Sendable {
    var comments: [PostComment] = []
}

struct NewComment: Codable, Equatable, Sendable {
    var text: String
}

struct LocalCommentFile: Codable, Equatable, Sendable {
    var posts: [String: [LocalComment]] = [:]
}

struct LocalComment: Codable, Equatable, Identifiable, Sendable {
    var id: String
    var text: String
    var createdAt: Int64
}

struct PostHit: Equatable, Hashable, Identifiable, Sendable {
    var book: BookRecord
    var post: FeedPost
    var id: String { "\(book.id):\(post.id)" }
}

struct SearchResult: Equatable, Sendable {
    var books: [BookRecord] = []
    var posts: [PostHit] = []
}

struct ProfileReplyRecord: Codable, Equatable, Sendable {
    var bookId: String
    var postId: String
    var comment: PostComment
}

struct ProfileReplies: Codable, Equatable, Sendable {
    var username: String = ""
    var replies: [ProfileReplyRecord] = []
}

struct ProfileReply: Equatable, Hashable, Sendable {
    var book: BookRecord
    var post: FeedPost
    var comment: PostComment
}

struct AuthRequest: Codable, Sendable {
    var username: String
    var password: String
}

struct AuthResponse: Codable, Sendable {
    var token: String
    var username: String
}

struct HostedErrorBody: Codable, Sendable {
    var error: String = ""
}

/// Reading place, or an owner-only share change. Omitted fields stay as they are.
struct PatchBook: Codable, Sendable {
    var progressIndex: Int?
    var shared: Bool?

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encodeIfPresent(progressIndex, forKey: .progressIndex)
        try container.encodeIfPresent(shared, forKey: .shared)
    }
}

/// Active hosted shelf. Finished books of either kind stay in one section.
struct HostedShelfGroups: Equatable, Sendable {
    var mine: [BookRecord] = []
    var sharedWithMe: [BookRecord] = []
    var finished: [BookRecord] = []
}

func hostedShelfGroups(_ books: [BookRecord]) -> HostedShelfGroups {
    let finished = books.filter(\.isFinished)
    let active = books.filter { !$0.isFinished }
    return HostedShelfGroups(
        mine: active.filter(\.mine),
        sharedWithMe: active.filter { !$0.mine },
        finished: finished
    )
}

func ownerCredit(_ book: BookRecord) -> String? {
    guard !book.mine, let name = book.ownerUsername?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty else {
        return nil
    }
    return "\(book.shared ? "Shared" : "Added") by @\(name)"
}

func privateUploadNotice(for book: BookRecord) -> String? {
    guard !book.isSample, !book.shared else { return nil }
    return "\(book.title) is private."
}

let usernameRule = "3–32 characters: letters, digits, underscore."
let passwordRule = "At least 8 characters."

func usernameFieldError(_ value: String) -> String? {
    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.isEmpty { return nil }
    if trimmed.range(of: #"^[A-Za-z0-9_]{3,32}$"#, options: .regularExpression) != nil { return nil }
    return usernameRule
}

func passwordFieldError(_ value: String) -> String? {
    if value.isEmpty || value.count >= 8 { return nil }
    return passwordRule
}

func hostedAuthReady(username: String, password: String, baseUrl: String) -> Bool {
    let name = username.trimmingCharacters(in: .whitespacesAndNewlines)
    return name.range(of: #"^[A-Za-z0-9_]{3,32}$"#, options: .regularExpression) != nil
        && password.count >= 8
        && !normalizeBaseUrl(baseUrl).isEmpty
}

func commentBudgetError(_ draft: String) -> String? {
    let asked = draft.trimmingCharacters(in: .whitespacesAndNewlines).count
    guard asked > maxCommentChars else { return nil }
    return "Comment budget: asked \(asked) characters, limit is \(maxCommentChars)."
}

/// Last server value for a share chain, and the value on screen.
struct ShareToggle: Equatable, Sendable {
    var confirmed: Bool
    var displayed: Bool
}

enum ShareToggleStart: Equatable, Sendable {
    case ignore
    case forbidden
    case update(ShareToggle)
}

func startShareToggle(book: BookRecord, requested: Bool, pending: ShareToggle?) -> ShareToggleStart {
    guard book.mine else { return .forbidden }
    if book.shared == requested && pending == nil { return .ignore }
    return .update(ShareToggle(confirmed: pending?.confirmed ?? book.shared, displayed: requested))
}

/// Keep a newer toggle on screen. Returns nil when this response is stale.
func applyShareSuccess(book: BookRecord, requested: Bool, server: BookRecord) -> BookRecord? {
    guard book.shared == requested else { return nil }
    var next = book
    next.shared = server.shared
    next.mine = server.mine
    next.ownerUsername = server.ownerUsername
    return next
}

/// Roll back only the toggle that failed. A newer choice stays put.
func applyShareFailure(book: BookRecord, requested: Bool, confirmed: Bool) -> BookRecord? {
    guard book.shared == requested else { return nil }
    var next = book
    next.shared = confirmed
    return next
}

struct PostsResponse: Codable, Sendable {
    var posts: [FeedPost] = []
}

let feedCacheVersion = 4
let maxCommentChars = 2000
let localCommentName = "You"
let hostedNotSignedIn = "Sign in to the hosted library first."

func slug(_ value: String) -> String {
    let cleaned = value.lowercased()
        .replacingOccurrences(of: "[^a-z0-9]+", with: "", options: .regularExpression)
        .prefix(18)
    return cleaned.isEmpty ? "author" : String(cleaned)
}

func nowMillis() -> Int64 {
    Int64(Date().timeIntervalSince1970 * 1000)
}

func relativeTime(createdAt: Int64, now: Int64 = nowMillis()) -> String {
    let delta = max(0, now - createdAt)
    let minutes = delta / 60_000
    if minutes < 1 { return "now" }
    if minutes < 60 { return "\(minutes)m" }
    let hours = minutes / 60
    if hours < 24 { return "\(hours)h" }
    return "\(hours / 24)d"
}

func quoteLine(author: String?, title: String?, text: String) -> String {
    let by = [author, title].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ", ")
    return by.isEmpty ? text : "\(text)\n\n— \(by)"
}

func statusLine(for book: BookRecord) -> String {
    let count = book.postCount > 0 ? "\(book.postCount) posts" : "Not opened yet"
    if book.isFinished { return "\(count) · Finished" }
    if book.postCount > 0 && book.progressIndex > 0 {
        let pct = min(100, max(1, (book.progressIndex + 1) * 100 / book.postCount))
        return "\(count) · \(pct)%"
    }
    return count
}
