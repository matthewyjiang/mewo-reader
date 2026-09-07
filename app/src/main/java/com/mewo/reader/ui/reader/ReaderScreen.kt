package com.mewo.reader.ui.reader

import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Palette
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.currentHeadingIndex
import com.mewo.reader.data.headingPosts
import com.mewo.reader.ui.components.MewoAppBar
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import java.io.File

@Composable
fun ReaderScreen(
    viewModel: ReaderViewModel,
    onBack: () -> Unit,
    onDisplay: () -> Unit,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val book = state.book
    var showChapters by rememberSaveable { mutableStateOf(false) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        ReaderBar(
            title = book?.title ?: "Post",
            onBack = onBack,
            onDisplay = onDisplay,
            onChapters = {
                if (!state.loading && state.error == null) showChapters = true
            },
        )
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)

        when {
            state.loading -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(
                        color = MaterialTheme.colorScheme.primary,
                        strokeWidth = 2.dp,
                        modifier = Modifier.size(22.dp),
                    )
                }
            }
            state.error != null -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(state.error ?: "", style = MaterialTheme.typography.bodyLarge)
                        TextButton(onClick = onBack) {
                            Text("Back", color = MaterialTheme.colorScheme.primary)
                        }
                    }
                }
            }
            else -> {
                val listState = rememberLazyListState(
                    initialFirstVisibleItemIndex = state.startIndex,
                )
                val scope = rememberCoroutineScope()
                val headings = remember(state.posts) { state.posts.headingPosts() }
                val openChapters = { showChapters = true }
                LaunchedEffect(listState) {
                    snapshotFlow { listState.firstVisibleItemIndex }
                        .drop(1)
                        .distinctUntilChanged()
                        .collect { viewModel.saveProgress(it) }
                }
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize(),
                ) {
                    itemsIndexed(state.posts, key = { _, post -> post.id }) { _, post ->
                        TimelinePost(
                            post = post,
                            author = book?.author ?: "Author",
                            handle = book?.handle ?: "author",
                            cover = book?.let { File(context.filesDir, "books/${it.id}/cover.jpg") },
                            liked = post.id in state.likes,
                            onLike = { viewModel.toggleLike(post.id) },
                            onRepost = {
                                shareText(context, quote(book?.author, book?.title, post))
                            },
                            onShare = {
                                shareText(context, post.text)
                            },
                            onChapterClick = openChapters,
                        )
                    }
                    item {
                        EndOfFeed(title = book?.title)
                    }
                }
                if (showChapters) {
                    val current = currentHeadingIndex(
                        posts = state.posts,
                        headings = headings,
                        visibleIndex = listState.firstVisibleItemIndex,
                    )
                    ChapterSheet(
                        title = book?.title ?: "Chapters",
                        headings = headings,
                        currentIndex = current,
                        onJump = { heading ->
                            showChapters = false
                            val index = state.posts.indexOfFirst { it.id == heading.id }
                            if (index >= 0) {
                                scope.launch { listState.animateScrollToItem(index) }
                            }
                        },
                        onDismiss = { showChapters = false },
                    )
                }
            }
        }
    }
}

@Composable
private fun ReaderBar(
    title: String,
    onBack: () -> Unit,
    onDisplay: () -> Unit,
    onChapters: () -> Unit,
) {
    MewoAppBar(
        leading = {
            IconButton(onClick = onBack) {
                Icon(
                    Icons.AutoMirrored.Outlined.ArrowBack,
                    contentDescription = "Back",
                    tint = MaterialTheme.colorScheme.onBackground,
                )
            }
        },
        center = {
            Box(
                modifier = Modifier
                    .fillMaxHeight()
                    .padding(horizontal = 56.dp)
                    .clickable(role = Role.Button, onClick = onChapters)
                    .semantics {
                        contentDescription = "Chapters"
                        role = Role.Button
                    },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        },
        trailing = {
            IconButton(onClick = onDisplay) {
                Icon(
                    Icons.Outlined.Palette,
                    contentDescription = "Display",
                    tint = MaterialTheme.colorScheme.onBackground,
                )
            }
        },
    )
}

@Composable
private fun EndOfFeed(title: String?) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 24.dp, vertical = 48.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = if (title != null) "End of $title" else "You're caught up",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
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
