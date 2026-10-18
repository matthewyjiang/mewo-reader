package com.mewo.reader.ui.profile

import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.ProfilePostRecord
import com.mewo.reader.data.ProfileRecord
import com.mewo.reader.data.ProfileRepostRecord
import com.mewo.reader.data.hasReposted
import com.mewo.reader.data.joinedLabel
import com.mewo.reader.data.repostCount
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalHostedAuth
import com.mewo.reader.ui.LocalLibraryStore
import com.mewo.reader.ui.commenterIdentity
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.components.BindListTop
import com.mewo.reader.ui.components.ErrorStatus
import com.mewo.reader.ui.components.HideOnScrollState
import com.mewo.reader.ui.components.LoadWhenNearEnd
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ShowWhenIdle
import com.mewo.reader.ui.components.SpinnerStatus
import com.mewo.reader.ui.components.UnderlineTabs
import com.mewo.reader.ui.currentReaderIdentity
import com.mewo.reader.ui.library.BookPost
import com.mewo.reader.ui.reader.CommentItem
import com.mewo.reader.ui.reader.CommentSheet
import com.mewo.reader.ui.reader.CommentTarget
import com.mewo.reader.ui.reader.TimelinePost
import com.mewo.reader.ui.reader.relativeTime

@Composable
fun ProfileScreen(
    viewModel: ProfileViewModel,
    displayName: String,
    onBack: () -> Unit,
    onOpenPost: (bookId: String, postId: String) -> Unit,
    onOpenBook: (String) -> Unit,
    onOpenProfile: (handle: String, name: String) -> Unit,
    hideOnScroll: HideOnScrollState,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val hosted = LocalBackend.current.kind == BackendKind.Hosted
    val token = LocalHostedAuth.current.session.token
    val sessionId = if (hosted) token else "local"
    LaunchedEffect(sessionId) { viewModel.bindSession(sessionId) }
    val me = currentReaderIdentity()
    val viewingSelf = state.handle.equals(me.handle, ignoreCase = true)
    val name = state.record?.username ?: if (viewingSelf) me.name else displayName.ifBlank { state.handle }
    val handle = state.record?.username ?: if (viewingSelf) me.handle else state.handle
    var commentTarget by remember { mutableStateOf<CommentTarget?>(null) }
    LaunchedEffect(state.handle) { viewModel.refresh() }

    val listEmpty = when (state.tab) {
        ProfileTab.Posts -> state.posts.isEmpty()
        ProfileTab.Reposts -> state.reposts.isEmpty()
        ProfileTab.Books -> state.books.isEmpty()
    }
    val cursor = when (state.tab) {
        ProfileTab.Posts -> state.postsCursor
        ProfileTab.Reposts -> state.repostsCursor
        ProfileTab.Books -> null
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        hideOnScroll.ShowWhenIdle(state.missing || listEmpty)
        ProfileBar(
            name = name,
            posts = state.postCount,
            visible = hideOnScroll.visible,
            onBack = onBack,
        )
        if (state.missing) {
            Text(
                text = "This account doesn't exist",
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 24.dp),
            )
            return@Column
        }
        val listState = rememberLazyListState()
        hideOnScroll.BindListTop(listState)
        LoadWhenNearEnd(
            listState = listState,
            enabled = cursor != null && !state.loading && !state.more,
            onLoad = viewModel::loadMore,
        )
        LazyColumn(
            state = listState,
            modifier = Modifier
                .fillMaxSize()
                .nestedScroll(hideOnScroll.connection),
        ) {
            item(key = "header") {
                ProfileHeader(name = name, handle = handle, record = state.record)
            }
            item(key = "tabs") {
                UnderlineTabs(
                    tabs = profileTabs,
                    selected = state.tab.name.lowercase(),
                    onSelect = { id ->
                        val tab = when (id) {
                            "reposts" -> ProfileTab.Reposts
                            "books" -> ProfileTab.Books
                            else -> ProfileTab.Posts
                        }
                        viewModel.select(tab)
                    },
                )
            }
            if (state.loading && listEmpty) {
                item(key = "loading") { SpinnerStatus() }
            }
            if (state.error != null && listEmpty) {
                item(key = "error") {
                    ErrorStatus(
                        message = state.error ?: "Could not load this profile.",
                        onRetry = viewModel::refresh,
                    )
                }
            }
            if (state.tab == ProfileTab.Posts) {
                items(state.posts, key = { "${it.book.id}:${it.post.id}:${it.comment.id}" }) { hit ->
                    val identity = commenterIdentity(hit.comment)
                    ProfileReplyRow(
                        hit = hit,
                        replyName = identity.name,
                        replyHandle = identity.handle,
                        liked = state.likes[hit.book.id]?.contains(hit.post.id) == true,
                        commentCount = state.comments[hit.book.id]?.count(hit.post.id) ?: 0,
                        commented = state.comments[hit.book.id]?.commented(hit.post.id) == true,
                        reposted = state.comments[hit.book.id]?.hasReposted(hit.post.id) == true,
                        repostCount = state.comments[hit.book.id]?.repostCount(hit.post.id) ?: 0,
                        onComment = { commentTarget = CommentTarget(hit.book, hit.post) },
                        onLike = { viewModel.toggleLike(hit.book.id, hit.post.id) },
                        onRepost = { viewModel.toggleRepost(hit.book.id, hit.post.id) },
                        onQuote = { shareText(context, quote(hit.book.author, hit.book.title, hit.post)) },
                        onShare = { shareText(context, hit.post.text) },
                        onOpenPassage = { onOpenPost(hit.book.id, hit.post.id) },
                        onOpenReply = { commentTarget = CommentTarget(hit.book, hit.post) },
                        onOpenProfile = onOpenProfile,
                    )
                }
            }
            if (state.tab == ProfileTab.Reposts) {
                items(state.reposts, key = { "repost-${it.book.id}:${it.post.id}" }) { hit ->
                    ProfileRepostRow(
                        hit = hit,
                        liked = state.likes[hit.book.id]?.contains(hit.post.id) == true,
                        commentCount = state.comments[hit.book.id]?.count(hit.post.id) ?: 0,
                        commented = state.comments[hit.book.id]?.commented(hit.post.id) == true,
                        reposted = state.comments[hit.book.id]?.hasReposted(hit.post.id) == true,
                        repostCount = state.comments[hit.book.id]?.repostCount(hit.post.id) ?: 0,
                        onComment = { commentTarget = CommentTarget(hit.book, hit.post) },
                        onLike = { viewModel.toggleLike(hit.book.id, hit.post.id) },
                        onRepost = { viewModel.toggleRepost(hit.book.id, hit.post.id) },
                        onQuote = { shareText(context, quote(hit.book.author, hit.book.title, hit.post)) },
                        onShare = { shareText(context, hit.post.text) },
                        onOpen = { onOpenPost(hit.book.id, hit.post.id) },
                    )
                }
            }
            if (state.tab == ProfileTab.Books) {
                items(state.books, key = { "book-${it.id}" }) { book ->
                    ProfileBookRow(book = book, onOpen = { onOpenBook(book.id) })
                }
            }
            if (state.more) {
                item(key = "more") { SpinnerStatus() }
            }
            if (!state.loading && state.error == null && listEmpty) {
                item(key = "empty") {
                    EmptyPosts(if (state.tab == ProfileTab.Books) "No books" else "No posts")
                }
            }
        }
        commentTarget?.let { target ->
            CommentSheet(
                target = target,
                publicNotes = hosted,
                onThreadChanged = { postId, comments ->
                    viewModel.onCommentsChanged(target.book.id, postId, comments)
                },
                onDismiss = { commentTarget = null },
                onOpenProfile = { nextHandle, nextName ->
                    commentTarget = null
                    onOpenProfile(nextHandle, nextName)
                },
            )
        }
    }
}

private val profileTabs = listOf(
    "posts" to "Posts",
    "reposts" to "Reposts",
    "books" to "Books",
)

@Composable
private fun ProfileBar(
    name: String,
    posts: Int?,
    visible: Boolean,
    onBack: () -> Unit,
) {
    MewoAppBar(
        visible = visible,
        leading = {
            IconButton(onClick = onBack) {
                Icon(
                    Icons.AutoMirrored.Outlined.ArrowBack,
                    contentDescription = "Back",
                    tint = MaterialTheme.colorScheme.onBackground,
                )
            }
        },
        content = {
            Column(Modifier.padding(end = 16.dp)) {
                Text(
                    text = name,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                )
                if (posts != null) {
                    Text(
                        text = if (posts == 1) "1 post" else "$posts posts",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
    )
}

@Composable
internal fun ProfileHeader(
    name: String,
    handle: String,
    record: ProfileRecord?,
) {
    Box(Modifier.fillMaxWidth()) {
        Column(Modifier.fillMaxWidth()) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(125.dp)
                    .background(MaterialTheme.colorScheme.surfaceVariant),
            )
            Spacer(Modifier.height(42.dp))
            Text(
                text = name,
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
            Text(
                text = "@$handle",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
            if (record != null && record.joinedAt > 0) {
                Text(
                    text = joinedLabel(record.joinedAt),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            }
            Spacer(Modifier.height(16.dp))
        }
        Box(
            modifier = Modifier
                .padding(start = 12.dp, top = 87.dp)
                .size(76.dp)
                .clip(CircleShape)
                .background(MaterialTheme.colorScheme.background),
            contentAlignment = Alignment.Center,
        ) {
            Avatar(name = name, size = 68.dp)
        }
    }
}

@Composable
private fun ProfileReplyRow(
    hit: ProfilePostRecord,
    replyName: String,
    replyHandle: String,
    liked: Boolean,
    commentCount: Int,
    commented: Boolean,
    reposted: Boolean,
    repostCount: Int,
    onComment: () -> Unit,
    onLike: () -> Unit,
    onRepost: () -> Unit,
    onQuote: () -> Unit,
    onShare: () -> Unit,
    onOpenPassage: () -> Unit,
    onOpenReply: () -> Unit,
    onOpenProfile: (handle: String, name: String) -> Unit,
) {
    CommentItem(
        author = hit.book.author,
        handle = hit.book.handle,
        post = hit.post,
        cover = LocalLibraryStore.current.coverFile(hit.book.id),
        replyName = replyName,
        replyHandle = replyHandle,
        replyMeta = relativeTime(hit.comment.createdAt),
        replyText = hit.comment.text,
        liked = liked,
        commentCount = commentCount,
        commented = commented,
        reposted = reposted,
        repostCount = repostCount,
        onOpen = onOpenPassage,
        onOpenReply = onOpenReply,
        onOpenReplyProfile = onOpenProfile,
        onComment = onComment,
        onLike = onLike,
        onRepost = onRepost,
        onQuote = onQuote,
        onShare = onShare,
    )
}

@Composable
private fun ProfileRepostRow(
    hit: ProfileRepostRecord,
    liked: Boolean,
    commentCount: Int,
    commented: Boolean,
    reposted: Boolean,
    repostCount: Int,
    onComment: () -> Unit,
    onLike: () -> Unit,
    onRepost: () -> Unit,
    onQuote: () -> Unit,
    onShare: () -> Unit,
    onOpen: () -> Unit,
) {
    val library = LocalLibraryStore.current
    TimelinePost(
        post = hit.post,
        author = hit.book.author,
        handle = hit.book.handle,
        cover = library.coverFile(hit.book.id),
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
private fun ProfileBookRow(book: BookRecord, onOpen: () -> Unit) {
    val library = LocalLibraryStore.current
    val hosted = LocalBackend.current.kind == BackendKind.Hosted
    BookPost(
        book = book,
        cover = library.coverFile(book.id),
        showOwner = hosted && !book.mine,
        onOpen = onOpen,
        onDelete = null,
        onShare = null,
    )
    HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
}

@Composable
internal fun EmptyPosts(label: String = "No posts") {
    Text(
        text = label,
        style = MaterialTheme.typography.headlineLarge,
        modifier = Modifier.padding(horizontal = 28.dp, vertical = 48.dp),
    )
}

private fun quote(author: String?, title: String?, post: FeedPost): String {
    val by = listOfNotNull(author, title).joinToString(", ")
    return if (by.isBlank()) post.text else "${post.text}\n\n— $by"
}

private fun shareText(context: android.content.Context, text: String) {
    val intent = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_TEXT, text)
    }
    context.startActivity(Intent.createChooser(intent, null))
}
