package com.mewo.reader.ui.likes

import android.content.Intent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.FeedPost
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.reader.TimelinePost
import java.io.File

@Composable
fun LikesScreen(
    viewModel: LikesViewModel,
    onOpenPost: (bookId: String, postId: String) -> Unit,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    LaunchedEffect(Unit) { viewModel.refresh() }

    Column(Modifier.fillMaxSize()) {
        MewoAppBar(
            content = {
                Text(
                    text = "Likes",
                    style = MaterialTheme.typography.titleLarge,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            },
        )
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)

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
                LazyColumn(Modifier.fillMaxSize()) {
                    items(state.hits, key = { "${it.book.id}:${it.post.id}" }) { hit ->
                        TimelinePost(
                            post = hit.post,
                            author = hit.book.author,
                            handle = hit.book.handle,
                            cover = File(context.filesDir, "books/${hit.book.id}/cover.jpg"),
                            liked = true,
                            onLike = { viewModel.unlike(hit.book.id, hit.post.id) },
                            onRepost = {
                                shareText(context, quote(hit.book.author, hit.book.title, hit.post))
                            },
                            onShare = { shareText(context, hit.post.text) },
                            onOpen = { onOpenPost(hit.book.id, hit.post.id) },
                        )
                    }
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
