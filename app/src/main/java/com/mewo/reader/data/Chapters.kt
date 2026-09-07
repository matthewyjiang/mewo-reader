package com.mewo.reader.data

fun List<FeedPost>.headingPosts(): List<FeedPost> =
    filter { it.kind == PostKind.Heading }

/**
 * Index in [headings] of the last heading at or before [visibleIndex] in [posts].
 * Used so the chapter sheet can mark and scroll to where you already are.
 */
fun currentHeadingIndex(
    posts: List<FeedPost>,
    headings: List<FeedPost>,
    visibleIndex: Int,
): Int {
    if (headings.isEmpty()) return 0
    val indexById = HashMap<String, Int>(posts.size)
    posts.forEachIndexed { index, post -> indexById[post.id] = index }
    val limit = visibleIndex
    return headings.indices.lastOrNull { i ->
        (indexById[headings[i].id] ?: Int.MAX_VALUE) <= limit
    } ?: 0
}
