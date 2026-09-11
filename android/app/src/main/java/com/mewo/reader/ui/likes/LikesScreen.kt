package com.mewo.reader.ui.likes

import android.content.Intent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.FeedPost
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalLibraryStore
import com.mewo.reader.ui.components.BindListTop
import com.mewo.reader.ui.components.HideOnScrollState
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileButton
import com.mewo.reader.ui.components.ShowWhenIdle
import com.mewo.reader.ui.reader.CommentSheet
import com.mewo.reader.ui.reader.CommentTarget
import com.mewo.reader.ui.reader.TimelinePost

@Composable
fun LikesScreen(
    viewModel: LikesViewModel,
    onOpenPost: (bookId: String, postId: String) -> Unit,
    onOpenAccount: () -> Unit,
    onOpenProfile: (handle: String, name: String) -> Unit,
    hideOnScroll: HideOnScrollState,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val library = LocalLibraryStore.current
    val hosted = LocalBackend.current.kind == BackendKind.Hosted
    var commentTarget by remember { mutableStateOf<CommentTarget?>(null) }
    LaunchedEffect(Unit) { viewModel.refresh() }

    Column(Modifier.fillMaxSize()) {
        hideOnScroll.ShowWhenIdle(state.loading || state.hits.isEmpty())
        MewoAppBar(
            visible = hideOnScroll.visible,
            leading = { ProfileButton(onClick = onOpenAccount) },
            content = {
                Text(
                    text = "Likes",
                    style = MaterialTheme.typography.titleLarge,
                    modifier = Modifier.padding(end = 16.dp),
                )
            },
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
            state.hits.isEmpty() -> {
                Text(
                    text = "Heart a line while you read. It shows up here.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
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
                    items(state.hits, key = { "${it.book.id}:${it.post.id}" }) { hit ->
                        TimelinePost(
                            post = hit.post,
                            author = hit.book.author,
                            handle = hit.book.handle,
                            cover = library.coverFile(hit.book.id),
                            liked = true,
                            commentCount = state.comments[hit.book.id]?.count(hit.post.id) ?: 0,
                            commented = state.comments[hit.book.id]?.commented(hit.post.id) == true,
                            onComment = { commentTarget = CommentTarget(hit.book, hit.post) },
                            onLike = { viewModel.unlike(hit.book.id, hit.post.id) },
                            onRepost = {
                                shareText(context, quote(hit.book.author, hit.book.title, hit.post))
                            },
                            onShare = { shareText(context, hit.post.text) },
                            onOpen = { onOpenPost(hit.book.id, hit.post.id) },
                        )
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
                        onOpenProfile = { handle, name ->
                            commentTarget = null
                            onOpenProfile(handle, name)
                        },
                    )
                }
            }
        }
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
