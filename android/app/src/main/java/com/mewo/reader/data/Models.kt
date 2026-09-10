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
)

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

data class PostHit(
    val book: BookRecord,
    val post: FeedPost,
)

data class SearchResult(
    val books: List<BookRecord> = emptyList(),
    val posts: List<PostHit> = emptyList(),
)
