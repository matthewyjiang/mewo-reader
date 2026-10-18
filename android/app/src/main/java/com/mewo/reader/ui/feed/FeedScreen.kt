package com.mewo.reader.ui.feed

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Repeat
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPhase
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.HomeFeedItem
import com.mewo.reader.data.hasReposted
import com.mewo.reader.data.repostCount
import com.mewo.reader.data.showFeedEmpty
import com.mewo.reader.ui.LocalLibraryStore
import com.mewo.reader.ui.components.BindListTop
import com.mewo.reader.ui.components.ErrorStatus
import com.mewo.reader.ui.components.HideOnScrollState
import com.mewo.reader.ui.components.LoadWhenNearEnd
import com.mewo.reader.ui.components.SpinnerStatus
import com.mewo.reader.ui.reader.CommentItem
import com.mewo.reader.ui.reader.CommentTarget
import com.mewo.reader.ui.reader.TimelinePost
import com.mewo.reader.ui.reader.relativeTime

@Composable
fun HomeFeed(
    phase: FeedPhase,
    likes: Map<String, Set<String>>,
    comments: Map<String, CommentIndex>,
    hideOnScroll: HideOnScrollState,
    onOpenPost: (bookId: String, postId: String) -> Unit,
    onOpenActor: (username: String) -> Unit,
    onComment: (CommentTarget) -> Unit,
    onLike: (bookId: String, postId: String) -> Unit,
    onRepost: (bookId: String, postId: String) -> Unit,
    onQuote: (BookRecord, FeedPost) -> Unit,
    onShare: (FeedPost) -> Unit,
    onRetry: () -> Unit,
    onLoadMore: () -> Unit,
) {
    val page = when (phase) {
        FeedPhase.Loading -> null
        is FeedPhase.Loaded -> phase.page
        is FeedPhase.Failed -> phase.page
    }
    val failed = phase as? FeedPhase.Failed
    val loaded = phase as? FeedPhase.Loaded
    val listState = rememberLazyListState()
    hideOnScroll.BindListTop(listState)
    LoadWhenNearEnd(
        listState = listState,
        enabled = loaded != null && !loaded.refreshing && !loaded.more && loaded.page.nextCursor != null,
        onLoad = onLoadMore,
    )
    LazyColumn(
        state = listState,
        modifier = Modifier
            .fillMaxSize()
            .nestedScroll(hideOnScroll.connection),
    ) {
        if (phase is FeedPhase.Loading) {
            item(key = "feed-loading") { SpinnerStatus() }
        }
        failed?.let { error ->
            item(key = "feed-error") {
                ErrorStatus(message = error.error, onRetry = onRetry)
            }
        }
        page?.let { shown ->
            items(shown.items, key = { itemKey(it) }) { item ->
                HomeFeedRow(
                    item = item,
                    likes = likes,
                    comments = comments,
                    onOpenPost = onOpenPost,
                    onOpenActor = onOpenActor,
                    onComment = onComment,
                    onLike = onLike,
                    onRepost = onRepost,
                    onQuote = onQuote,
                    onShare = onShare,
                )
            }
        }
        if (loaded?.more == true) {
            item(key = "feed-more") { SpinnerStatus() }
        }
        if (showFeedEmpty(phase)) {
            item(key = "feed-empty") { FeedEmpty() }
        }
    }
}

@Composable
internal fun HomeFeedRow(
    item: HomeFeedItem,
    likes: Map<String, Set<String>>,
    comments: Map<String, CommentIndex>,
    onOpenPost: (bookId: String, postId: String) -> Unit,
    onOpenActor: (username: String) -> Unit,
    onComment: (CommentTarget) -> Unit,
    onLike: (bookId: String, postId: String) -> Unit,
    onRepost: (bookId: String, postId: String) -> Unit,
    onQuote: (BookRecord, FeedPost) -> Unit,
    onShare: (FeedPost) -> Unit,
) {
    val index = comments[item.book.id]
    when (item) {
        is HomeFeedItem.Repost -> Column(Modifier.fillMaxWidth()) {
            ActivityLine(username = item.actor.username, onOpen = { onOpenActor(item.actor.username) })
            Passage(
                book = item.book,
                post = item.post,
                liked = likes[item.book.id]?.contains(item.post.id) == true,
                commentCount = index?.count(item.post.id) ?: 0,
                commented = index?.commented(item.post.id) == true,
                reposted = index?.hasReposted(item.post.id) == true,
                repostCount = index?.repostCount(item.post.id) ?: 0,
                onOpen = { onOpenPost(item.book.id, item.post.id) },
                onComment = { onComment(CommentTarget(item.book, item.post)) },
                onLike = { onLike(item.book.id, item.post.id) },
                onRepost = { onRepost(item.book.id, item.post.id) },
                onQuote = { onQuote(item.book, item.post) },
                onShare = { onShare(item.post) },
            )
        }
        is HomeFeedItem.Comment -> CommentItem(
            author = item.book.author,
            handle = item.book.handle,
            post = item.post,
            cover = LocalLibraryStore.current.coverFile(item.book.id),
            replyName = item.actor.username,
            replyHandle = item.actor.username,
            replyMeta = relativeTime(item.comment.createdAt),
            replyText = item.comment.text,
            liked = likes[item.book.id]?.contains(item.post.id) == true,
            commentCount = index?.count(item.post.id) ?: 0,
            commented = index?.commented(item.post.id) == true,
            reposted = index?.hasReposted(item.post.id) == true,
            repostCount = index?.repostCount(item.post.id) ?: 0,
            onOpen = { onOpenPost(item.book.id, item.post.id) },
            onOpenReply = { onComment(CommentTarget(item.book, item.post)) },
            onOpenReplyProfile = { handle, _ -> onOpenActor(handle) },
            onComment = { onComment(CommentTarget(item.book, item.post)) },
            onLike = { onLike(item.book.id, item.post.id) },
            onRepost = { onRepost(item.book.id, item.post.id) },
            onQuote = { onQuote(item.book, item.post) },
            onShare = { onShare(item.post) },
        )
    }
}

@Composable
internal fun ActivityLine(username: String, onOpen: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(role = Role.Button, onClick = onOpen)
            .padding(start = 68.dp, end = 16.dp, top = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = Icons.Outlined.Repeat,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(16.dp),
        )
        Spacer(Modifier.width(8.dp))
        Text(
            text = "$username reposted",
            style = MaterialTheme.typography.labelSmall.copy(
                fontWeight = FontWeight.Bold,
                fontSize = 13.sp,
            ),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun Passage(
    book: BookRecord,
    post: FeedPost,
    liked: Boolean,
    commentCount: Int,
    commented: Boolean,
    reposted: Boolean,
    repostCount: Int,
    onOpen: () -> Unit,
    onComment: () -> Unit,
    onLike: () -> Unit,
    onRepost: () -> Unit,
    onQuote: () -> Unit,
    onShare: () -> Unit,
) {
    TimelinePost(
        post = post,
        author = book.author,
        handle = book.handle,
        cover = LocalLibraryStore.current.coverFile(book.id),
        liked = liked,
        commentCount = commentCount,
        commented = commented,
        reposted = reposted,
        repostCount = repostCount,
        onComment = onComment,
        onLike = onLike,
        onRepost = onRepost,
        onQuote = onQuote,
        onShare = onShare,
        onOpen = onOpen,
    )
}

@Composable
internal fun FeedEmpty() {
    Text(
        text = "Nothing here yet",
        style = MaterialTheme.typography.headlineLarge,
        modifier = Modifier.padding(horizontal = 28.dp, vertical = 48.dp),
    )
}

private fun itemKey(item: HomeFeedItem): String = when (item) {
    is HomeFeedItem.Comment -> "comment:${item.at}:${item.comment.id}:${item.book.id}:${item.post.id}"
    is HomeFeedItem.Repost -> "repost:${item.at}:${item.actor.username}:${item.book.id}:${item.post.id}"
}
