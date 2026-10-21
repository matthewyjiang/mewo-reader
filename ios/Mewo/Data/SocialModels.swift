import Foundation

enum SearchTab: String, Codable, CaseIterable, Identifiable, Sendable {
    case accounts, books, posts, catalog

    var id: String { rawValue }

    var title: String {
        switch self {
        case .accounts: return "Accounts"
        case .books: return "Books"
        case .posts: return "Posts"
        case .catalog: return "Catalog"
        }
    }
}

/// Catalog is a different endpoint. It is never a `type` on `/v1/search`.
func librarySearchType(_ tab: SearchTab) -> String? {
    switch tab {
    case .accounts, .books, .posts: return tab.rawValue
    case .catalog: return nil
    }
}

func searchTabs(hosted: Bool, canRequest: Bool) -> [SearchTab] {
    if !hosted { return [.books, .posts] }
    return canRequest ? [.accounts, .books, .posts, .catalog] : [.accounts, .books, .posts]
}

struct AccountRecord: Codable, Equatable, Hashable, Identifiable, Sendable {
    var username: String
    var mine: Bool = false
    var id: String { username }

    enum CodingKeys: String, CodingKey { case username, mine }
}

struct SearchPage: Equatable, Sendable {
    var type: SearchTab
    var accounts: [AccountRecord] = []
    var books: [BookRecord] = []
    var posts: [PostHit] = []
    var nextCursor: String?
}

extension SearchPage: Decodable {
    enum CodingKeys: String, CodingKey { case type, items, nextCursor }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        type = try container.decode(SearchTab.self, forKey: .type)
        nextCursor = try container.decodeIfPresent(String.self, forKey: .nextCursor)
        switch type {
        case .accounts:
            accounts = try container.decode([AccountRecord].self, forKey: .items)
        case .books:
            books = try container.decode([BookRecord].self, forKey: .items)
        case .posts:
            posts = try container.decode([PostHit].self, forKey: .items)
        case .catalog:
            throw DecodingError.dataCorruptedError(
                forKey: .type,
                in: container,
                debugDescription: "Catalog is not a library search page."
            )
        }
    }
}

struct ProfileRecord: Codable, Equatable, Sendable {
    var username: String
    var joinedAt: Int64
    var postCount: Int
    var repostCount: Int
    var mine: Bool
}

struct ProfilePostItem: Codable, Equatable, Sendable {
    var book: BookRecord
    var post: FeedPost
    var comment: PostComment
}

struct ProfilePostsPage: Codable, Equatable, Sendable {
    var items: [ProfilePostItem] = []
    var nextCursor: String?
}

struct ProfileRepostItem: Codable, Equatable, Identifiable, Sendable {
    var book: BookRecord
    var post: FeedPost
    var repostedAt: Int64
    var id: String { "\(book.id):\(post.id):\(repostedAt)" }
}

struct ProfileRepostsPage: Codable, Equatable, Sendable {
    var items: [ProfileRepostItem] = []
    var nextCursor: String?
}

struct ProfileBooksPage: Codable, Equatable, Sendable {
    var items: [BookRecord] = []
}

struct RepostState: Codable, Equatable, Sendable {
    var reposts: Int
    var reposted: Bool
}

struct RepostMark: Equatable, Sendable {
    var count: Int
    var on: Bool
}

struct ActorRecord: Codable, Equatable, Hashable, Sendable {
    var username: String
}

struct HomeFeedItem: Equatable, Hashable, Identifiable, Sendable {
    var type: String
    var actor: String
    var book: BookRecord
    var post: FeedPost
    var comment: PostComment?
    var at: Int64

    init(type: String, actor: String, book: BookRecord, post: FeedPost, comment: PostComment?, at: Int64) {
        self.type = type
        self.actor = actor
        self.book = book
        self.post = post
        self.comment = comment
        self.at = at
    }

    var id: String {
        if type == "comment" {
            return "comment:\(comment?.id ?? "")"
        }
        return "repost:\(actor):\(book.id):\(post.id):\(at)"
    }

    var isRepost: Bool { type == "repost" }
}

extension HomeFeedItem: Codable {
    enum CodingKeys: String, CodingKey { case type, actor, book, post, comment, at }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        type = try container.decode(String.self, forKey: .type)
        actor = try container.decode(ActorRecord.self, forKey: .actor).username
        book = try container.decode(BookRecord.self, forKey: .book)
        post = try container.decode(FeedPost.self, forKey: .post)
        comment = try container.decodeIfPresent(PostComment.self, forKey: .comment)
        at = try container.decode(Int64.self, forKey: .at)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(type, forKey: .type)
        try container.encode(ActorRecord(username: actor), forKey: .actor)
        try container.encode(book, forKey: .book)
        try container.encode(post, forKey: .post)
        try container.encodeIfPresent(comment, forKey: .comment)
        try container.encode(at, forKey: .at)
    }
}

struct HomeFeedPage: Codable, Equatable, Sendable {
    var items: [HomeFeedItem] = []
    var nextCursor: String?
}

let missingAccountCopy = "This account doesn't exist"
let emptyFeedCopy = "Nothing here yet"
let noMatchesCopy = "No matches"
let searchDebounceNanos: UInt64 = 250_000_000

func optimisticRepost(_ mark: RepostMark) -> RepostMark {
    if mark.on {
        return RepostMark(count: max(0, mark.count - 1), on: false)
    }
    return RepostMark(count: mark.count + 1, on: true)
}

/// In-flight reposts for one book. A load paints the server index, then these
/// marks are written back onto their own posts. Other posts keep the counts
/// the load just fetched. The server answer applies while this toggle still
/// owns the post, even if a load put the old count back on screen.
@MainActor
final class RepostOverlay {
    private var marks: [String: RepostMark] = [:]

    func begin(postId: String, mark: RepostMark) {
        marks[postId] = mark
    }

    func replay(over index: CommentIndex) -> CommentIndex {
        var next = index
        for (postId, mark) in marks {
            next = next.applyingRepost(postId, mark)
        }
        return next
    }

    func finish(
        current: CommentIndex,
        postId: String,
        sent: RepostMark,
        previous: RepostMark,
        result: Result<RepostState, Error>
    ) -> CommentIndex {
        guard marks[postId] == sent else { return current }
        marks.removeValue(forKey: postId)
        switch result {
        case .success(let state):
            return current.applyingRepost(postId, RepostMark(count: state.reposts, on: state.reposted))
        case .failure:
            return current.applyingRepost(postId, previous)
        }
    }
}

@MainActor
@discardableResult
func commitRepostToggle(
    bookId: String,
    postId: String,
    read: @escaping @MainActor () -> CommentIndex,
    library: LibraryStore,
    publish: @escaping @MainActor (CommentIndex) -> Void,
    overlay: RepostOverlay
) -> Task<Void, Never> {
    let previous = read().repostMark(postId)
    let sent = optimisticRepost(previous)
    overlay.begin(postId: postId, mark: sent)
    publish(overlay.replay(over: read()))
    return Task {
        let result: Result<RepostState, Error>
        do {
            result = .success(try await library.setRepost(id: bookId, postId: postId, on: sent.on))
        } catch {
            result = .failure(error)
        }
        publish(overlay.finish(
            current: read(),
            postId: postId,
            sent: sent,
            previous: previous,
            result: result
        ))
    }
}

func joinedLabel(_ millis: Int64) -> String {
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.timeZone = TimeZone(secondsFromGMT: 0)
    formatter.dateFormat = "MMM yyyy"
    let date = Date(timeIntervalSince1970: TimeInterval(millis) / 1000)
    return "Joined \(formatter.string(from: date))"
}

func postCountLabel(_ count: Int) -> String {
    count == 1 ? "1 post" : "\(count) posts"
}
