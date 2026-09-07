package com.mewo.reader.ui.search

import android.content.Intent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.FeedPost
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.reader.TimelinePost
import java.io.File

@Composable
fun SearchScreen(
    viewModel: SearchViewModel,
    onOpenBook: (String) -> Unit,
    onOpenPost: (bookId: String, postId: String) -> Unit,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }

    Column(Modifier.fillMaxSize()) {
        MewoAppBar(
            content = {
                BasicTextField(
                    value = state.query,
                    onValueChange = viewModel::setQuery,
                    singleLine = true,
                    textStyle = MaterialTheme.typography.bodyLarge.merge(
                        TextStyle(color = MaterialTheme.colorScheme.onBackground),
                    ),
                    cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                    modifier = Modifier
                        .weight(1f)
                        .padding(horizontal = 16.dp)
                        .focusRequester(focus),
                    decorationBox = { inner ->
                        if (state.query.isEmpty()) {
                            Text(
                                "Search books and posts",
                                style = MaterialTheme.typography.bodyLarge,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        inner()
                    },
                )
            },
        )
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)

        when {
            state.query.isBlank() -> {
                Hint("Type a title, author, or a line you remember.")
            }
            state.result.books.isEmpty() && state.result.posts.isEmpty() -> {
                Hint("No matches for \"${state.query}\".")
            }
            else -> {
                LazyColumn(Modifier.fillMaxSize()) {
                    if (state.result.books.isNotEmpty()) {
                        item { SectionLabel("Books") }
                        items(state.result.books, key = { "book-${it.id}" }) { book ->
                            BookHit(
                                book = book,
                                cover = File(context.filesDir, "books/${book.id}/cover.jpg"),
                                onOpen = { onOpenBook(book.id) },
                            )
                            HorizontalDivider(
                                color = MaterialTheme.colorScheme.outline,
                                thickness = 0.6.dp,
                            )
                        }
                    }
                    if (state.result.posts.isNotEmpty()) {
                        item { SectionLabel("Posts") }
                        items(state.result.posts, key = { "post-${it.book.id}-${it.post.id}" }) { hit ->
                            TimelinePost(
                                post = hit.post,
                                author = hit.book.author,
                                handle = hit.book.handle,
                                cover = File(context.filesDir, "books/${hit.book.id}/cover.jpg"),
                                liked = state.likes[hit.book.id]?.contains(hit.post.id) == true,
                                onLike = { viewModel.toggleLike(hit.book.id, hit.post.id) },
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
}

@Composable
private fun SectionLabel(text: String) {
    Text(
        text = text,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 10.dp),
    )
}

@Composable
private fun Hint(text: String) {
    Text(
        text = text,
        style = MaterialTheme.typography.bodyLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 24.dp),
    )
}

@Composable
private fun BookHit(
    book: BookRecord,
    cover: File,
    onOpen: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(role = Role.Button, onClick = onOpen)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Avatar(name = book.author, coverFile = cover, size = 40.dp)
        Spacer(Modifier.width(12.dp))
        Column {
            Text(book.title, style = MaterialTheme.typography.titleSmall)
            Spacer(Modifier.height(2.dp))
            Text(
                text = "@${book.handle}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
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
