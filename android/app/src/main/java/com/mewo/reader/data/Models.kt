package com.mewo.reader.data

import kotlinx.serialization.Serializable
import kotlinx.serialization.Transient

@Serializable
data class LibrarySnapshot(
    val books: List<BookRecord> = emptyList(),
)

@Serializable
data class BookRecord(
    val id: String,
    val title: String,
    val author: String,
    val handle: String,
    val importedAt: Long,
    val postCount: Int = 0,
    val progressIndex: Int = 0,
    val isSample: Boolean = false,
    val mine: Boolean = true,
    /** Owner chose to show this book on other accounts' shelves. */
    val shared: Boolean = false,
    /** Username of the account that added the book. Older servers omit it. */
    val ownerUsername: String? = null,
) {
    /**
     * Saved place is the last post. Index 0 is the start, so an unopened
     * book stays on the timeline. A one-post book finishes when the reader
     * scrolls onto the end card and the index moves past 0.
     */
    val isFinished: Boolean
        get() = postCount > 0 && progressIndex >= maxOf(1, postCount - 1)
}

@Serializable
data class FeedCache(
    val version: Int = 1,
    val posts: List<FeedPost> = emptyList(),
)

@Serializable
data class FeedPost(
    val id: String,
    val kind: PostKind,
    val text: String,
    val chapter: String,
)

@Serializable
enum class PostKind {
    Heading,
    Body,
}

@Serializable
data class LikeSet(
    val ids: Set<String> = emptySet(),
)

@Serializable
data class CommentIndex(
    val counts: Map<String, Int> = emptyMap(),
    val mine: Set<String> = emptySet(),
    /** Posts with at least one repost. Zero counts are omitted, same as [counts]. */
    val reposts: Map<String, Int> = emptyMap(),
    /** Post ids this viewer has reposted. Same shape as [mine]. */
    val reposted: Set<String> = emptySet(),
) {
    fun count(postId: String): Int = counts[postId] ?: 0

    fun commented(postId: String): Boolean = postId in mine

    fun afterThread(postId: String, comments: List<PostComment>): CommentIndex {
        val nextCounts = counts.toMutableMap()
        if (comments.isEmpty()) nextCounts.remove(postId)
        else nextCounts[postId] = comments.size
        val nextMine = mine.toMutableSet()
        if (comments.any { it.mine }) nextMine.add(postId) else nextMine.remove(postId)
        return copy(counts = nextCounts, mine = nextMine)
    }
}

/** Where a reply is relative to the server. Server JSON never includes this. */
@Serializable
sealed interface CommentStatus {
    @Serializable
    data object Sent : CommentStatus

    @Serializable
    data object Sending : CommentStatus

    @Serializable
    data class Failed(val message: String) : CommentStatus
}

@Serializable
data class PostComment(
    val id: String,
    val username: String,
    val text: String,
    val createdAt: Long,
    val mine: Boolean = false,
    /**
     * Client-only. Kept out of [equals] and [hashCode] so a thread diff against
     * the server does not treat Sending or Failed as a different reply.
     */
    @Transient
    val status: CommentStatus = CommentStatus.Sent,
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is PostComment) return false
        return id == other.id &&
            username == other.username &&
            text == other.text &&
            createdAt == other.createdAt &&
            mine == other.mine
    }

    override fun hashCode(): Int {
        var result = id.hashCode()
        result = 31 * result + username.hashCode()
        result = 31 * result + text.hashCode()
        result = 31 * result + createdAt.hashCode()
        result = 31 * result + mine.hashCode()
        return result
    }
}

@Serializable
data class CommentThread(
    val comments: List<PostComment> = emptyList(),
)

/** Per-book offline availability. Downloaded and NotDownloaded come from files. */
sealed interface OfflineState {
    data object NotDownloaded : OfflineState

    /** [fraction] is null until the server sends Content-Length. */
    data class Downloading(val fraction: Float?) : OfflineState

    data object Downloaded : OfflineState

    data class Failed(val message: String) : OfflineState
}

/** Shelf-wide status for the Settings row. Derived, never stored. */
data class OfflineSummary(
    val downloaded: Int,
    val total: Int,
    val running: Boolean,
)

const val SOCIAL_CACHE_VERSION = 1

/** Last social state the server returned for a book. Read only when the network fails. */
@Serializable
data class SocialCache(
    val version: Int = SOCIAL_CACHE_VERSION,
    val likes: Set<String> = emptySet(),
    val index: CommentIndex = CommentIndex(),
    /** Threads opened for this book, by post id. Not prefetched. */
    val threads: Map<String, List<PostComment>> = emptyMap(),
)

@Serializable
data class NewComment(
    val text: String,
    /** Lowercase client UUID. A replay with this id returns the same thread. */
    val id: String? = null,
    /** Only ever raises privacy. The server ORs this with the insert trigger. */
    val `private`: Boolean = false,
)

@Serializable
data class LocalCommentFile(
    val posts: Map<String, List<LocalComment>> = emptyMap(),
)

@Serializable
data class LocalComment(
    val id: String,
    val text: String,
    val createdAt: Long,
)

data class PostHit(
    val book: BookRecord,
    val post: FeedPost,
)

data class SearchResult(
    val books: List<BookRecord> = emptyList(),
    val posts: List<PostHit> = emptyList(),
)

@Serializable
data class ProfileReplyRecord(
    val bookId: String,
    val postId: String,
    val comment: PostComment,
)

@Serializable
data class ProfileReplies(
    val username: String = "",
    val replies: List<ProfileReplyRecord> = emptyList(),
)

data class ProfileReply(
    val book: BookRecord,
    val post: FeedPost,
    val comment: PostComment,
)

@Serializable
data class RepostState(
    val reposts: Int = 0,
    val reposted: Boolean = false,
)

enum class SearchKind {
    Accounts,
    Books,
    Posts,
    /** Open Library. Never sent as `type` on `/v1/search`. */
    Catalog,
    ;

    val wire: String get() = name.lowercase()
}

@Serializable
data class AccountHit(
    val username: String,
    val mine: Boolean = false,
)

@Serializable
sealed class SearchPage {
    @Serializable
    @kotlinx.serialization.SerialName("accounts")
    data class Accounts(
        val items: List<AccountHit> = emptyList(),
        val nextCursor: String? = null,
    ) : SearchPage()

    @Serializable
    @kotlinx.serialization.SerialName("books")
    data class Books(
        val items: List<BookRecord> = emptyList(),
        val nextCursor: String? = null,
    ) : SearchPage()

    @Serializable
    @kotlinx.serialization.SerialName("posts")
    data class Posts(
        val items: List<PostHitRecord> = emptyList(),
        val nextCursor: String? = null,
    ) : SearchPage()
}

@Serializable
data class PostHitRecord(
    val book: BookRecord,
    val post: FeedPost,
)

fun SearchPage.cursor(): String? = when (this) {
    is SearchPage.Accounts -> nextCursor
    is SearchPage.Books -> nextCursor
    is SearchPage.Posts -> nextCursor
}

fun SearchPage.isEmpty(): Boolean = when (this) {
    is SearchPage.Accounts -> items.isEmpty()
    is SearchPage.Books -> items.isEmpty()
    is SearchPage.Posts -> items.isEmpty()
}

fun SearchPage.kind(): SearchKind = when (this) {
    is SearchPage.Accounts -> SearchKind.Accounts
    is SearchPage.Books -> SearchKind.Books
    is SearchPage.Posts -> SearchKind.Posts
}

@Serializable
data class ProfileRecord(
    val username: String,
    val joinedAt: Long = 0,
    val postCount: Int = 0,
    val repostCount: Int = 0,
    val mine: Boolean = false,
)

@Serializable
data class ProfilePostRecord(
    val book: BookRecord,
    val post: FeedPost,
    val comment: PostComment,
)

@Serializable
data class ProfilePostsPage(
    val items: List<ProfilePostRecord> = emptyList(),
    val nextCursor: String? = null,
)

@Serializable
data class ProfileRepostRecord(
    val book: BookRecord,
    val post: FeedPost,
    val repostedAt: Long = 0,
)

@Serializable
data class ProfileRepostsPage(
    val items: List<ProfileRepostRecord> = emptyList(),
    val nextCursor: String? = null,
)

@Serializable
data class ProfileBooks(
    val items: List<BookRecord> = emptyList(),
)

@Serializable
data class ActorRecord(
    val username: String,
)

@Serializable
sealed class HomeFeedItem {
    abstract val actor: ActorRecord
    abstract val book: BookRecord
    abstract val post: FeedPost
    abstract val at: Long

    @Serializable
    @kotlinx.serialization.SerialName("comment")
    data class Comment(
        override val actor: ActorRecord,
        override val book: BookRecord,
        override val post: FeedPost,
        val comment: PostComment,
        override val at: Long,
    ) : HomeFeedItem()

    @Serializable
    @kotlinx.serialization.SerialName("repost")
    data class Repost(
        override val actor: ActorRecord,
        override val book: BookRecord,
        override val post: FeedPost,
        override val at: Long,
    ) : HomeFeedItem()
}

@Serializable
data class HomeFeedPage(
    val items: List<HomeFeedItem> = emptyList(),
    val nextCursor: String? = null,
)

/** `Joined Mon YYYY` from `users.created_at` milliseconds, in UTC. */
fun joinedLabel(joinedAt: Long): String {
    val date = java.time.Instant.ofEpochMilli(joinedAt).atZone(java.time.ZoneOffset.UTC)
    val month = date.month.getDisplayName(java.time.format.TextStyle.SHORT, java.util.Locale.US)
    return "Joined $month ${date.year}"
}
