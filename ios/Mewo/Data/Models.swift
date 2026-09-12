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

struct PatchBook: Codable, Sendable {
    var postCount: Int?
    var progressIndex: Int?

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encodeIfPresent(postCount, forKey: .postCount)
        try container.encodeIfPresent(progressIndex, forKey: .progressIndex)
    }
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
    if book.postCount > 0 && book.progressIndex > 0 {
        let pct = min(100, max(1, (book.progressIndex + 1) * 100 / book.postCount))
        return "\(count) · \(pct)%"
    }
    return count
}
