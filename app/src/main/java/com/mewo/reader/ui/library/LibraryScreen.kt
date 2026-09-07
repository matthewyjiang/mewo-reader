package com.mewo.reader.ui.library

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Snackbar
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.BookRecord
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.theme.XBlue
import java.io.File

@Composable
fun LibraryScreen(
    viewModel: LibraryViewModel,
    onOpenBook: (String) -> Unit,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val picker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri: Uri? ->
        if (uri != null) viewModel.import(uri)
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        Column(Modifier.fillMaxSize()) {
            HomeBar()
            HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
            if (state.books.isEmpty() && !state.busy) {
                EmptyTimeline(
                    onAdd = { picker.launch(arrayOf("application/epub+zip", "application/octet-stream")) },
                    onSample = viewModel::importSample,
                )
            } else {
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(bottom = 96.dp),
                ) {
                    items(state.books, key = { it.id }) { book ->
                        BookPost(
                            book = book,
                            cover = File(context.filesDir, "books/${book.id}/cover.jpg"),
                            onOpen = { onOpenBook(book.id) },
                            onDelete = { viewModel.delete(book.id) },
                        )
                        HorizontalDivider(
                            color = MaterialTheme.colorScheme.outline,
                            thickness = 0.6.dp,
                        )
                    }
                }
            }
        }

        FloatingActionButton(
            onClick = { picker.launch(arrayOf("application/epub+zip", "application/octet-stream")) },
            containerColor = XBlue,
            contentColor = MaterialTheme.colorScheme.onPrimary,
            shape = CircleShape,
            modifier = Modifier
                .align(Alignment.BottomEnd)
                .navigationBarsPadding()
                .padding(end = 16.dp, bottom = 16.dp)
                .size(56.dp),
        ) {
            Icon(Icons.Outlined.Add, contentDescription = "Add an EPUB")
        }

        if (state.busy) {
            CircularProgressIndicator(
                color = XBlue,
                strokeWidth = 2.dp,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .navigationBarsPadding()
                    .padding(bottom = 24.dp)
                    .size(22.dp),
            )
        }

        state.error?.let { message ->
            Snackbar(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .navigationBarsPadding()
                    .padding(16.dp),
                action = {
                    TextButton(onClick = viewModel::dismissError) {
                        Text("OK")
                    }
                },
            ) {
                Text(message)
            }
        }
    }
}

@Composable
private fun HomeBar() {
    MewoAppBar {
        Text(
            text = "Mewo",
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(horizontal = 16.dp),
        )
    }
}

@Composable
private fun EmptyTimeline(
    onAdd: () -> Unit,
    onSample: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 28.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.Start,
    ) {
        Text(
            text = "Your timeline is empty",
            style = MaterialTheme.typography.headlineLarge,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            text = "Drop in an EPUB and the author starts posting, one paragraph at a time.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(20.dp))
        TextButton(
            onClick = onAdd,
            modifier = Modifier
                .clip(RoundedCornerShape(999.dp))
                .background(XBlue)
                .padding(horizontal = 8.dp),
        ) {
            Text("Add a book", color = MaterialTheme.colorScheme.onPrimary)
        }
        TextButton(onClick = onSample) {
            Text("Open a sample", color = XBlue)
        }
    }
}

@Composable
private fun BookPost(
    book: BookRecord,
    cover: File,
    onOpen: () -> Unit,
    onDelete: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(role = Role.Button, onClick = onOpen)
            .padding(horizontal = 16.dp, vertical = 12.dp),
    ) {
        Avatar(name = book.author, coverFile = cover, size = 40.dp)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = book.title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                    modifier = Modifier.weight(1f, fill = false),
                )
                Spacer(Modifier.width(6.dp))
                Text(
                    text = "@${book.handle}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                )
            }
            Spacer(Modifier.height(2.dp))
            Text(
                text = statusLine(book),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
        IconButton(onClick = onDelete, modifier = Modifier.size(40.dp)) {
            Icon(
                Icons.Outlined.Delete,
                contentDescription = "Remove ${book.title}",
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

private fun statusLine(book: BookRecord): String {
    val count = if (book.postCount > 0) "${book.postCount} posts" else "Not opened yet"
    val progress = if (book.postCount > 0 && book.progressIndex > 0) {
        val pct = ((book.progressIndex + 1) * 100 / book.postCount).coerceIn(1, 100)
        " · $pct%"
    } else {
        ""
    }
    return "$count$progress"
}
