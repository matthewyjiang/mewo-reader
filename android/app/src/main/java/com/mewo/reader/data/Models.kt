package com.mewo.reader.data

import kotlinx.serialization.Serializable

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

@Serializable
data class PostComment(
    val id: String,
    val username: String,
    val text: String,
    val createdAt: Long,
    val mine: Boolean = false,
)

@Serializable
data class CommentThread(
    val comments: List<PostComment> = emptyList(),
)

@Serializable
data class NewComment(
    val text: String,
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
