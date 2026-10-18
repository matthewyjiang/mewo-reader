package com.mewo.reader.ui.library

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.People
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.FeedPhase
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.FileHomeFeedCache
import com.mewo.reader.data.LibraryFailed
import com.mewo.reader.data.LibraryLoading
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.cachedBooks
import com.mewo.reader.data.isRefreshing
import com.mewo.reader.data.showEmpty
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalHostedAuth
import com.mewo.reader.ui.LocalLibraryStore
import com.mewo.reader.ui.components.UnderlineTabs
import com.mewo.reader.ui.feed.FeedViewModel
import com.mewo.reader.ui.feed.HomeFeed
import com.mewo.reader.ui.reader.CommentSheet
import com.mewo.reader.ui.reader.CommentTarget
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.components.BindListTop
import com.mewo.reader.ui.components.BrandMark
import com.mewo.reader.ui.components.HideOnScrollState
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileButton
import com.mewo.reader.ui.components.ShowWhenIdle
import java.io.File
import kotlin.math.roundToInt
import androidx.lifecycle.viewmodel.compose.viewModel as composeViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(
    viewModel: LibraryViewModel,
    onOpenBook: (String) -> Unit,
    onOpenPost: (bookId: String, postId: String) -> Unit,
    onOpenAccount: () -> Unit,
    onOpenProfile: (handle: String, name: String) -> Unit,
    hideOnScroll: HideOnScrollState,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val load = state.load
    val books = load.cachedBooks().orEmpty()
    val library = LocalLibraryStore.current
    val hosted = LocalBackend.current.kind == BackendKind.Hosted
    val context = LocalContext.current
    val auth = LocalHostedAuth.current
    var home by rememberSaveable { mutableStateOf(if (hosted) "feed" else "books") }
    // The feed is server-only. A saved "feed" tab from a hosted session opens on Books.
    LaunchedEffect(hosted) {
        if (!hosted) home = "books"
    }
    val onBooks = !hosted || home == "books"
    val feedCache = remember { FileHomeFeedCache(File(context.filesDir, "home-feeds")) }
    val feedViewModel: FeedViewModel = composeViewModel(
        factory = FeedViewModel.factory(library, feedCache),
    )
    val feedState by feedViewModel.state.collectAsStateWithLifecycle()
    val feedSession = feedSessionKey(hosted, auth.session.signedIn, auth.session.normalizedUrl(), auth.session.username)
    LaunchedEffect(feedSession) { feedViewModel.bindSession(feedSession) }
    var commentTarget by remember { mutableStateOf<CommentTarget?>(null) }
    var pendingDelete by remember { mutableStateOf<BookRecord?>(null) }
    val picker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri: Uri? ->
        if (uri != null) viewModel.import(uri, hosted)
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        Column(Modifier.fillMaxSize()) {
            HomeBar(onOpenAccount = onOpenAccount, visible = hideOnScroll.visible)
            UnderlineTabs(
                tabs = if (hosted) listOf("feed" to "Feed", "books" to "Books") else listOf("books" to "Books"),
                selected = if (hosted) home else "books",
                onSelect = { home = it },
            )
            val feedItems = when (val phase = feedState.phase) {
                FeedPhase.Loading -> emptyList()
                is FeedPhase.Loaded -> phase.page.items
                is FeedPhase.Failed -> phase.page?.items.orEmpty()
            }
            hideOnScroll.ShowWhenIdle(if (onBooks) books.isEmpty() else feedItems.isEmpty())
            val pullState = rememberPullToRefreshState()
            val pullThreshold = PullToRefreshDefaults.PositionalThreshold
            val feedRefreshing = (feedState.phase as? FeedPhase.Loaded)?.refreshing == true
            PullToRefreshBox(
                isRefreshing = if (onBooks) load.isRefreshing() else feedRefreshing,
                onRefresh = { if (onBooks) viewModel.refresh() else feedViewModel.refresh() },
                state = pullState,
                modifier = Modifier.weight(1f).fillMaxWidth(),
                // The default indicator hangs over the first row. Open a gap the
                // same height and center the spinner in it; the app bar stays put.
                indicator = {
                    val gap = pullThreshold * pullState.distanceFraction
                    if (gap > 0.dp) {
                        Box(
                            modifier = Modifier
                                .align(Alignment.TopCenter)
                                .fillMaxWidth()
                                .height(gap)
                                .clipToBounds(),
                            contentAlignment = Alignment.Center,
                        ) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(22.dp),
                                strokeWidth = 2.dp,
                                color = MaterialTheme.colorScheme.primary,
                            )
                        }
                    }
                },
            ) {
                Box(
                    Modifier
                        .fillMaxSize()
                        .offset {
                            IntOffset(
                                x = 0,
                                y = (pullThreshold.toPx() * pullState.distanceFraction).roundToInt(),
                            )
                        },
                ) {
                if (!onBooks) {
                    HomeFeed(
                        phase = feedState.phase,
                        likes = feedState.likes,
                        comments = feedState.comments,
                        hideOnScroll = hideOnScroll,
                        onOpenPost = onOpenPost,
                        onOpenActor = { username -> onOpenProfile(username, username) },
                        onComment = { commentTarget = it },
                        onLike = feedViewModel::toggleLike,
                        onRepost = feedViewModel::toggleRepost,
                        onQuote = { book, post -> shareText(context, quote(book.author, book.title, post)) },
                        onShare = { post -> shareText(context, post.text) },
                        onRetry = feedViewModel::refresh,
                        onLoadMore = feedViewModel::loadMore,
                    )
                } else when {
                    load is LibraryLoading -> LibraryLoading()
                    load is LibraryFailed && load.books == null -> LibraryUnavailable(
                        message = load.message,
                        onRetry = viewModel::refresh,
                    )
                    load.showEmpty() -> EmptyTimeline(
                        onAdd = { picker.launch(arrayOf("application/epub+zip", "application/octet-stream")) },
                        onSample = viewModel::importSample,
                    )
                    else -> {
                        val listState = rememberLazyListState()
                        hideOnScroll.BindListTop(listState)
                        LazyColumn(
                            state = listState,
                            modifier = Modifier
                                .fillMaxSize()
                                .nestedScroll(hideOnScroll.connection),
                            contentPadding = PaddingValues(bottom = 96.dp),
                        ) {
                            if (hosted) {
                                val groups = shelfGroups(books)
                                if (groups.mine.isNotEmpty()) {
                                    item(key = "section-mine") { SectionLabel("My books") }
                                    bookRows(groups.mine, library, hosted, onOpenBook, { pendingDelete = it }, viewModel::setShared)
                                }
                                if (groups.sharedWithMe.isNotEmpty()) {
                                    item(key = "section-shared") { SectionLabel("Shared with me") }
                                    bookRows(groups.sharedWithMe, library, hosted, onOpenBook, { pendingDelete = it }, viewModel::setShared)
                                }
                                if (groups.finished.isNotEmpty()) {
                                    item(key = "section-finished") { SectionLabel("Finished") }
                                    bookRows(groups.finished, library, hosted, onOpenBook, { pendingDelete = it }, viewModel::setShared)
                                }
                            } else {
                                val reading = books.filterNot { it.isFinished }
                                val finished = books.filter { it.isFinished }
                                bookRows(reading, library, hosted = false, onOpenBook, { pendingDelete = it }, viewModel::setShared)
                                if (finished.isNotEmpty()) {
                                    item(key = "section-finished") { SectionLabel("Finished") }
                                    bookRows(finished, library, hosted = false, onOpenBook, { pendingDelete = it }, viewModel::setShared)
                                }
                            }
                        }
                    }
                }
                }
            }
        }

        if (onBooks) {
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

        val loadError = (load as? LibraryFailed)?.takeIf { it.books != null }?.message
        val notice = state.error ?: loadError
        notice?.let { message ->
            LibraryNotice(
                message = message,
                onDismiss = if (state.error != null) viewModel::dismissError else viewModel::dismissLoadError,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(16.dp),
            )
        }

        pendingDelete?.let { book ->
            RemoveBookDialog(
                book = book,
                hosted = hosted,
                onConfirm = {
                    viewModel.delete(book.id)
                    pendingDelete = null
                },
                onDismiss = { pendingDelete = null },
            )
        }

        commentTarget?.let { target ->
            CommentSheet(
                target = target,
                publicNotes = hosted,
                onThreadChanged = { postId, comments ->
                    feedViewModel.onCommentsChanged(target.book.id, postId, comments)
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

private fun feedSessionKey(
    hosted: Boolean,
    signedIn: Boolean,
    url: String?,
    username: String?,
): String? {
    if (!hosted) return "local"
    if (!signedIn || url.isNullOrBlank() || username.isNullOrBlank()) return null
    return "$url\n$username"
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

@Composable
internal fun LibraryNotice(
    message: String,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // App typography pins page ink; this inverse surface needs its own foreground.
    val foreground = SnackbarDefaults.contentColor
    // Keep Material's snackbar surface, but center arbitrary-length messages instead
    // of using its one-/two-line baseline offsets.
    Surface(
        modifier = modifier.widthIn(max = 600.dp),
        shape = SnackbarDefaults.shape,
        color = SnackbarDefaults.color,
        contentColor = foreground,
        shadowElevation = 6.dp,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = message,
                color = foreground,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.weight(1f).padding(top = 14.dp, bottom = 14.dp, end = 8.dp),
            )
            TextButton(onClick = onDismiss) {
                Text("OK", color = foreground)
            }
        }
    }
}

@Composable
internal fun RemoveBookDialog(
    book: BookRecord,
    hosted: Boolean,
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
                text = "Remove ${book.title}?",
                style = MaterialTheme.typography.titleLarge,
            )
        },
        text = {
            Text(
                text = removalCopy(book, hosted),
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

private fun removalCopy(book: BookRecord, hosted: Boolean): String {
    if (!hosted) return "Removes it from this phone."
    return if (book.shared || book.isSample) {
        "Removes it for everyone who can read it."
    } else {
        "Removes it from your library."
    }
}

private data class ShelfGroups(
    val mine: List<BookRecord>,
    val sharedWithMe: List<BookRecord>,
    val finished: List<BookRecord>,
)

private fun shelfGroups(books: List<BookRecord>): ShelfGroups {
    val finished = books.filter { it.isFinished }
    val active = books.filterNot { it.isFinished }
    return ShelfGroups(
        mine = active.filter { it.mine },
        sharedWithMe = active.filterNot { it.mine },
        finished = finished,
    )
}

@Composable
internal fun LibraryLoading() {
    // Start mid-arc so the first frame (and layout snapshots) show the ring, not an empty gap.
    val sweep = rememberInfiniteTransition(label = "shelf-loading").animateFloat(
        initialValue = 0.2f,
        targetValue = 0.8f,
        animationSpec = infiniteRepeatable(tween(durationMillis = 900), RepeatMode.Reverse),
        label = "shelf-loading-sweep",
    )
    QuietColumn {
        CircularProgressIndicator(
            progress = { sweep.value },
            modifier = Modifier.size(22.dp),
            color = MaterialTheme.colorScheme.primary,
            strokeWidth = 2.dp,
            trackColor = MaterialTheme.colorScheme.outline,
        )
    }
}

@Composable
internal fun LibraryUnavailable(
    message: String,
    onRetry: () -> Unit,
) {
    QuietColumn {
        Text(
            text = "Couldn't load",
            style = MaterialTheme.typography.headlineLarge,
        )
        Spacer(Modifier.height(12.dp))
        Text(
            text = message,
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        TextButton(
            onClick = onRetry,
            contentPadding = PaddingValues(0.dp),
        ) {
            Text("Try again", color = MaterialTheme.colorScheme.primary)
        }
    }
}

/** Same start inset and vertical center as [EmptyTimeline], tall enough to pull. */
@Composable
private fun QuietColumn(content: @Composable () -> Unit) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .height(maxHeight)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 28.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.Start,
        ) {
            content()
        }
    }
}

@Composable
internal fun EmptyTimeline(
    onAdd: () -> Unit,
    onSample: () -> Unit,
) {
    QuietColumn {
        Text(
            text = "No books",
            style = MaterialTheme.typography.headlineLarge,
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
        TextButton(
            onClick = onSample,
            contentPadding = PaddingValues(0.dp),
        ) {
            Text("Open a sample", color = MaterialTheme.colorScheme.primary)
        }
    }
}

private fun LazyListScope.bookRows(
    books: List<BookRecord>,
    library: LibraryStore,
    hosted: Boolean,
    onOpenBook: (String) -> Unit,
    onDelete: (BookRecord) -> Unit,
    onShare: (String, Boolean) -> Unit,
) {
    items(books, key = { it.id }) { book ->
        ShelfBook(
            book = book,
            cover = library.coverFile(book.id),
            showOwner = hosted && !book.mine,
            onOpen = { onOpenBook(book.id) },
            onDelete = if (book.mine) {
                { onDelete(book) }
            } else {
                null
            },
            onShare = if (hosted && book.mine) {
                { shared -> onShare(book.id, shared) }
            } else {
                null
            },
        )
    }
}

@Composable
internal fun SectionLabel(text: String) {
    Text(
        text = text,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 10.dp),
    )
}

@Composable
private fun ShelfBook(
    book: BookRecord,
    cover: File?,
    showOwner: Boolean,
    onOpen: () -> Unit,
    onDelete: (() -> Unit)?,
    onShare: ((Boolean) -> Unit)?,
) {
    BookPost(
        book = book,
        cover = cover,
        showOwner = showOwner,
        onOpen = onOpen,
        onDelete = onDelete,
        onShare = onShare,
    )
    HorizontalDivider(
        color = MaterialTheme.colorScheme.outline,
        thickness = 0.6.dp,
    )
}

@Composable
internal fun BookPost(
    book: BookRecord,
    cover: File?,
    showOwner: Boolean,
    onOpen: () -> Unit,
    onDelete: (() -> Unit)?,
    onShare: ((Boolean) -> Unit)?,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            modifier = Modifier
                .weight(1f)
                .clickable(role = Role.Button, onClick = onOpen)
                .padding(start = 16.dp, top = 12.dp, bottom = 12.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Avatar(name = book.author, coverFile = cover, size = 40.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        text = book.title,
                        style = MaterialTheme.typography.titleSmall,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    Spacer(Modifier.width(6.dp))
                    Text(
                        text = "@${book.handle}",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
                Spacer(Modifier.height(2.dp))
                Text(
                    text = statusLine(book),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                if (showOwner && !book.ownerUsername.isNullOrBlank()) {
                    Spacer(Modifier.height(2.dp))
                    Text(
                        text = if (book.shared) {
                            "Shared by @${book.ownerUsername}"
                        } else {
                            "Added by @${book.ownerUsername}"
                        },
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                    )
                }
            }
        }
        if (onShare != null || onDelete != null) {
            Row(
                modifier = Modifier.padding(end = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (onShare != null) {
                    val shared = book.shared
                    RowActionButton(
                        onClick = { onShare(!shared) },
                        icon = if (shared) Icons.Filled.People else Icons.Outlined.Lock,
                        contentDescription = if (shared) "Make private" else "Share",
                    )
                }
                if (onDelete != null) {
                    RowActionButton(
                        onClick = onDelete,
                        icon = Icons.Outlined.Delete,
                        contentDescription = "Remove ${book.title}",
                    )
                }
            }
        } else {
            Spacer(Modifier.width(16.dp))
        }
    }
}

@Composable
private fun RowActionButton(
    onClick: () -> Unit,
    icon: ImageVector,
    contentDescription: String,
) {
    IconButton(
        onClick = onClick,
        modifier = Modifier.size(RowActionSize),
    ) {
        Icon(
            imageVector = icon,
            contentDescription = contentDescription,
            modifier = Modifier.size(RowActionIconSize),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private val RowActionSize = 40.dp
private val RowActionIconSize = 24.dp

private fun statusLine(book: BookRecord): String {
    val count = if (book.postCount > 0) "${book.postCount} posts" else "Not opened yet"
    if (book.isFinished) return "$count · Finished"
    val progress = if (book.postCount > 0 && book.progressIndex > 0) {
        val pct = ((book.progressIndex + 1) * 100 / book.postCount).coerceIn(1, 100)
        " · $pct%"
    } else {
        ""
    }
    return "$count$progress"
}
