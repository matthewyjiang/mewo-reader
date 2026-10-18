package com.mewo.reader.data

fun CommentIndex.repostCount(postId: String): Int = reposts[postId] ?: 0

fun CommentIndex.hasReposted(postId: String): Boolean = postId in reposted

private fun Map<String, Int>.withCount(postId: String, count: Int): Map<String, Int> {
    val next = toMutableMap()
    if (count <= 0) next.remove(postId) else next[postId] = count
    return next
}

private fun Set<String>.withFlag(postId: String, on: Boolean): Set<String> {
    val next = toMutableSet()
    if (on) next.add(postId) else next.remove(postId)
    return next
}

/** One post flips. Other posts on the same book stay as they are. */
fun CommentIndex.applyRepost(postId: String, on: Boolean): CommentIndex {
    if (hasReposted(postId) == on) return this
    val count = repostCount(postId) + if (on) 1 else -1
    return copy(
        reposts = reposts.withCount(postId, count),
        reposted = reposted.withFlag(postId, on),
    )
}

fun CommentIndex.applyServerRepost(postId: String, state: RepostState): CommentIndex = copy(
    reposts = reposts.withCount(postId, state.reposts),
    reposted = reposted.withFlag(postId, state.reposted),
)

fun CommentIndex.rollbackRepost(before: CommentIndex, postId: String): CommentIndex = copy(
    reposts = reposts.withCount(postId, before.repostCount(postId)),
    reposted = reposted.withFlag(postId, before.hasReposted(postId)),
)

fun CommentIndex.sameRepost(other: CommentIndex, postId: String): Boolean =
    hasReposted(postId) == other.hasReposted(postId) && repostCount(postId) == other.repostCount(postId)

/**
 * Show the flipped count, then keep the server's answer. A failure puts the
 * post back unless a newer toggle already replaced it.
 */
suspend fun commitRepost(
    before: CommentIndex,
    postId: String,
    request: suspend (Boolean) -> RepostState,
    read: () -> CommentIndex,
    write: (CommentIndex) -> Unit,
) {
    val on = !before.hasReposted(postId)
    val optimistic = before.applyRepost(postId, on)
    write(optimistic)
    try {
        val state = request(on)
        val now = read()
        if (now.sameRepost(optimistic, postId)) write(now.applyServerRepost(postId, state))
    } catch (error: kotlinx.coroutines.CancellationException) {
        throw error
    } catch (error: Exception) {
        val now = read()
        if (now.sameRepost(optimistic, postId)) write(now.rollbackRepost(before, postId))
        throw error
    }
}
