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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
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
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.PostKind
import com.mewo.reader.data.ProfileReply
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalLibraryStore
import com.mewo.reader.ui.commenterIdentity
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.components.BindListTop
import com.mewo.reader.ui.components.HideOnScrollState
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ShowWhenIdle
import com.mewo.reader.ui.currentReaderIdentity
import com.mewo.reader.ui.reader.CommentSheet
import com.mewo.reader.ui.reader.CommentTarget
import com.mewo.reader.ui.reader.PostActions
import com.mewo.reader.ui.reader.ThreadPost
import com.mewo.reader.ui.reader.relativeTime

@Composable
fun ProfileScreen(
    viewModel: ProfileViewModel,
    displayName: String,
    onBack: () -> Unit,
    onOpenProfile: (handle: String, name: String) -> Unit,
    hideOnScroll: HideOnScrollState,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val hosted = LocalBackend.current.kind == BackendKind.Hosted
    val me = currentReaderIdentity()
    val viewingSelf = state.handle.equals(me.handle, ignoreCase = true)
    val name = if (viewingSelf) me.name else displayName.ifBlank { state.handle }
    val handle = if (viewingSelf) me.handle else state.handle
    var commentTarget by remember { mutableStateOf<CommentTarget?>(null) }
    LaunchedEffect(state.handle) { viewModel.refresh() }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        hideOnScroll.ShowWhenIdle(state.loading || state.replies.isEmpty())
        ProfileBar(
            name = name,
            posts = state.replies.size,
            visible = hideOnScroll.visible,
            onBack = onBack,
        )

        when {
            state.loading -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(
                        color = MaterialTheme.colorScheme.primary,
                        strokeWidth = 2.dp,
                        modifier = Modifier.padding(24.dp),
                    )
                }
            }
            state.error != null -> {
                Text(
                    text = state.error ?: "Could not load this profile.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 24.dp),
                )
            }
            else -> {
                val listState = rememberLazyListState()
                hideOnScroll.BindListTop(listState)
                LazyColumn(
                    state = listState,
                    modifier = Modifier
                        .fillMaxSize()
                        .nestedScroll(hideOnScroll.connection),
                ) {
                    item {
                        ProfileHeader(
                            name = name,
                            handle = handle,
                        )
                    }
                    if (state.replies.isEmpty()) {
                        item {
                            Text(
                                text = if (viewingSelf) {
                                    "Reply to a line. It shows up here."
                                } else {
                                    "No posts yet."
                                },
                                style = MaterialTheme.typography.bodyLarge,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.padding(horizontal = 16.dp, vertical = 24.dp),
                            )
                        }
                    } else {
                        items(
                            state.replies,
                            key = { "${it.book.id}:${it.post.id}:${it.comment.id}" },
                        ) { hit ->
                            val identity = commenterIdentity(hit.comment)
                            ProfileReplyRow(
                                hit = hit,
                                replyName = identity.name,
                                replyHandle = identity.handle,
                                liked = state.likes[hit.book.id]?.contains(hit.post.id) == true,
                                commentCount = state.comments[hit.book.id]?.count(hit.post.id) ?: 0,
                                commented = state.comments[hit.book.id]
                                    ?.commented(hit.post.id) == true,
                                onComment = { commentTarget = CommentTarget(hit.book, hit.post) },
                                onLike = { viewModel.toggleLike(hit.book.id, hit.post.id) },
                                onRepost = {
                                    shareText(
                                        context,
                                        quote(hit.book.author, hit.book.title, hit.post),
                                    )
                                },
                                onShare = { shareText(context, hit.post.text) },
                                onOpen = { commentTarget = CommentTarget(hit.book, hit.post) },
                                onOpenProfile = onOpenProfile,
                            )
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
    }
}

@Composable
private fun ProfileBar(
    name: String,
    posts: Int,
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
                Text(
                    text = if (posts == 1) "1 post" else "$posts posts",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
    )
}

@Composable
private fun ProfileHeader(
    name: String,
    handle: String,
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
            Spacer(Modifier.height(2.dp))
            Text(
                text = "@$handle",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
            Spacer(Modifier.height(16.dp))
            PostsTab()
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
private fun PostsTab() {
    Column(Modifier.fillMaxWidth()) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(48.dp),
            contentAlignment = Alignment.Center,
        ) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    text = "Posts",
                    style = MaterialTheme.typography.titleSmall,
                )
                Spacer(Modifier.height(10.dp))
                Box(
                    modifier = Modifier
                        .width(56.dp)
                        .height(3.dp)
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.primary),
                )
            }
        }
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
    }
}

@Composable
private fun ProfileReplyRow(
    hit: ProfileReply,
    replyName: String,
    replyHandle: String,
    liked: Boolean,
    commentCount: Int,
    commented: Boolean,
    onComment: () -> Unit,
    onLike: () -> Unit,
    onRepost: () -> Unit,
    onShare: () -> Unit,
    onOpen: () -> Unit,
    onOpenProfile: (handle: String, name: String) -> Unit,
) {
    val library = LocalLibraryStore.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(role = Role.Button, onClick = onOpen),
    ) {
        ThreadPost(
            name = hit.book.author,
            handle = hit.book.handle,
            meta = hit.post.chapter,
            text = hit.post.text,
            heading = hit.post.kind == PostKind.Heading,
            cover = library.coverFile(hit.book.id),
            connectDown = true,
        )
        ThreadPost(
            name = replyName,
            handle = replyHandle,
            meta = relativeTime(hit.comment.createdAt),
            text = hit.comment.text,
            heading = false,
            cover = null,
            connectDown = false,
            onOpenProfile = onOpenProfile,
        )
        Box(Modifier.padding(start = 68.dp, end = 12.dp)) {
            PostActions(
                liked = liked,
                commentCount = commentCount,
                commented = commented,
                onComment = onComment,
                onLike = onLike,
                onRepost = onRepost,
                onShare = onShare,
            )
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
    }
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
