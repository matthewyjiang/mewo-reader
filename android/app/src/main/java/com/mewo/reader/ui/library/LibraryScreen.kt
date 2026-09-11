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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material3.AlertDialog
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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.BookRecord
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalLibraryStore
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.components.BindListTop
import com.mewo.reader.ui.components.BrandMark
import com.mewo.reader.ui.components.HideOnScrollState
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileButton
import com.mewo.reader.ui.components.ShowWhenIdle
import java.io.File

@Composable
fun LibraryScreen(
    viewModel: LibraryViewModel,
    onOpenBook: (String) -> Unit,
    onOpenAccount: () -> Unit,
    hideOnScroll: HideOnScrollState,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val library = LocalLibraryStore.current
    val hosted = LocalBackend.current.kind == BackendKind.Hosted
    var pendingDelete by remember { mutableStateOf<BookRecord?>(null) }
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
            HomeBar(onOpenAccount = onOpenAccount, visible = hideOnScroll.visible)
            hideOnScroll.ShowWhenIdle(state.books.isEmpty())
            if (state.books.isEmpty() && !state.busy) {
                EmptyTimeline(
                    onAdd = { picker.launch(arrayOf("application/epub+zip", "application/octet-stream")) },
                    onSample = viewModel::importSample,
                )
            } else {
                val listState = rememberLazyListState()
                hideOnScroll.BindListTop(listState)
                LazyColumn(
                    state = listState,
                    modifier = Modifier
                        .fillMaxSize()
                        .nestedScroll(hideOnScroll.connection),
                    contentPadding = PaddingValues(bottom = 96.dp),
                ) {
                    items(state.books, key = { it.id }) { book ->
                        BookPost(
                            book = book,
                            cover = library.coverFile(book.id),
                            onOpen = { onOpenBook(book.id) },
                            onDelete = if (book.mine) {
                                { pendingDelete = book }
                            } else {
                                null
                            },
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
            containerColor = MaterialTheme.colorScheme.primary,
            contentColor = MaterialTheme.colorScheme.onPrimary,
            shape = CircleShape,
            modifier = Modifier
                .align(Alignment.BottomEnd)
                .padding(end = 16.dp, bottom = 16.dp)
                .size(56.dp),
        ) {
            Icon(Icons.Outlined.Add, contentDescription = "Add an EPUB")
        }

        if (state.busy) {
            CircularProgressIndicator(
                color = MaterialTheme.colorScheme.primary,
                strokeWidth = 2.dp,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(bottom = 24.dp)
                    .size(22.dp),
            )
        }

        state.error?.let { message ->
            Snackbar(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
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

        pendingDelete?.let { book ->
            RemoveBookDialog(
                book = book,
                shared = hosted,
                onConfirm = {
                    viewModel.delete(book.id)
                    pendingDelete = null
                },
                onDismiss = { pendingDelete = null },
            )
        }
    }
}

@Composable
private fun RemoveBookDialog(
    book: BookRecord,
    shared: Boolean,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = MaterialTheme.colorScheme.surface,
        titleContentColor = MaterialTheme.colorScheme.onBackground,
        textContentColor = MaterialTheme.colorScheme.onSurfaceVariant,
        title = {
            Text(
                text = "Remove from your library?",
                style = MaterialTheme.typography.headlineLarge,
            )
        },
        text = {
            Text(
                text = if (shared) {
                    "This removes ${book.title} for everyone on this server, plus likes, notes, and reading place."
                } else {
                    "This removes ${book.title}, plus likes, notes, and where you left off. You can add the EPUB again."
                },
                style = MaterialTheme.typography.bodyLarge,
            )
        },
        confirmButton = {
            TextButton(onClick = onConfirm) {
                Text("Remove", color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Keep", color = MaterialTheme.colorScheme.primary)
            }
        },
    )
}

@Composable
private fun HomeBar(onOpenAccount: () -> Unit, visible: Boolean) {
    MewoAppBar(
        visible = visible,
        leading = { ProfileButton(onClick = onOpenAccount) },
        center = { BrandMark() },
    )
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
                .background(MaterialTheme.colorScheme.primary)
                .padding(horizontal = 8.dp),
        ) {
            Text("Add a book", color = MaterialTheme.colorScheme.onPrimary)
        }
        TextButton(onClick = onSample) {
            Text("Open a sample", color = MaterialTheme.colorScheme.primary)
        }
    }
}

@Composable
private fun BookPost(
    book: BookRecord,
    cover: File?,
    onOpen: () -> Unit,
    onDelete: (() -> Unit)?,
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
        if (onDelete != null) {
            IconButton(onClick = onDelete, modifier = Modifier.size(40.dp)) {
                Icon(
                    Icons.Outlined.Delete,
                    contentDescription = "Remove ${book.title}",
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
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
