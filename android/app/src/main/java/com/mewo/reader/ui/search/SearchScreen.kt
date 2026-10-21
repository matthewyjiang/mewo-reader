package com.mewo.reader.ui.search

import android.content.Intent
import android.view.inputmethod.InputMethodManager
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.AccountHit
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CatalogAccess
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.PostHitRecord
import com.mewo.reader.data.SearchKind
import com.mewo.reader.data.SearchPage
import com.mewo.reader.data.cursor
import com.mewo.reader.data.hasReposted
import com.mewo.reader.data.repostCount
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalCatalogStore
import com.mewo.reader.ui.LocalHostedAuth
import com.mewo.reader.ui.LocalLibraryStore
import com.mewo.reader.ui.catalog.CatalogPhase
import com.mewo.reader.ui.catalog.CatalogResults
import com.mewo.reader.ui.catalog.RowOutcome
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.components.BindListTop
import com.mewo.reader.ui.components.ErrorStatus
import com.mewo.reader.ui.components.HideOnScrollState
import com.mewo.reader.ui.components.LoadWhenNearEnd
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileButton
import com.mewo.reader.ui.components.ShowWhenIdle
import com.mewo.reader.ui.components.SpinnerStatus
import com.mewo.reader.ui.components.UnderlineTabs
import com.mewo.reader.ui.library.BookPost
import com.mewo.reader.ui.reader.CommentSheet
import com.mewo.reader.ui.reader.CommentTarget
import com.mewo.reader.ui.reader.TimelinePost
import kotlinx.coroutines.launch

@Composable
fun SearchScreen(
    viewModel: SearchViewModel,
    onOpenBook: (String) -> Unit,
    onOpenPost: (bookId: String, postId: String) -> Unit,
    onOpenAccount: () -> Unit,
    onOpenProfile: (handle: String, name: String) -> Unit,
    hideOnScroll: HideOnScrollState,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val library = LocalLibraryStore.current
    val hosted = LocalBackend.current.kind == BackendKind.Hosted
    val token = LocalHostedAuth.current.session.token
    val sessionId = if (hosted) token else "local"
    LaunchedEffect(sessionId) { viewModel.bindSession(sessionId) }
    var commentTarget by remember { mutableStateOf<CommentTarget?>(null) }
    val focus = remember { FocusRequester() }
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    val view = LocalView.current
    LaunchedEffect(Unit) { focus.requestFocus() }
    LaunchedEffect(Unit) { viewModel.refreshSocial() }
    DisposableEffect(Unit) {
        onDispose {
            focusManager.clearFocus(force = true)
            keyboard?.hide()
            view.context.getSystemService(InputMethodManager::class.java)
                ?.hideSoftInputFromWindow(view.windowToken, 0)
        }
    }

    val search = state.search
    val page = visiblePage(search)
    val phase = search.phase
    val loading = phase is SearchPhase.Loading
    val failed = phase as? SearchPhase.Failed
    val loaded = phase as? SearchPhase.Loaded
    val canRequest = LocalCatalogStore.current.access.collectAsStateWithLifecycle().value == CatalogAccess.On
    val catalogState by viewModel.catalog.state.collectAsStateWithLifecycle()
    val tabs = searchTabs(hosted, canRequest)
    val scope = rememberCoroutineScope()
    val onCatalog = search.kind == SearchKind.Catalog

    Column(Modifier.fillMaxSize()) {
        MewoAppBar(
            leading = { ProfileButton(onClick = onOpenAccount) },
            content = {
                SearchField(
                    query = state.query,
                    onQueryChange = viewModel::setQuery,
                    hosted = hosted,
                    focusRequester = focus,
                    modifier = Modifier
                        .weight(1f)
                        .padding(end = 8.dp),
                )
            },
        )
        UnderlineTabs(
            tabs = tabs,
            selected = search.kind.wire,
            onSelect = { id ->
                val kind = when (id) {
                    "accounts" -> SearchKind.Accounts
                    "posts" -> SearchKind.Posts
                    "catalog" -> SearchKind.Catalog
                    else -> SearchKind.Books
                }
                viewModel.setKind(kind)
            },
        )
        val catalogIdle = onCatalog && (
            state.query.isBlank() ||
                catalogState.phase is CatalogPhase.Failed ||
                (catalogState.phase as? CatalogPhase.Loaded)?.page?.items?.isEmpty() == true
            )
        hideOnScroll.ShowWhenIdle(
            if (onCatalog) catalogIdle else state.query.isBlank() || page == null || page.isBlank(),
        )
        if (onCatalog) {
            CatalogResults(
                state = catalogState,
                query = state.query,
                hideOnScroll = hideOnScroll,
                onOpen = viewModel.catalog::open,
                onAct = { work ->
                    scope.launch {
                        when (val outcome = viewModel.catalog.act(work)) {
                            is RowOutcome.OpenBook -> onOpenBook(outcome.id)
                            RowOutcome.Done -> Unit
                        }
                    }
                },
                onRetry = viewModel.catalog::retry,
                onLoadMore = viewModel.catalog::loadMore,
                onDismissNotice = viewModel.catalog::dismissNotice,
                onCloseSheet = viewModel.catalog::close,
                onOpenBook = onOpenBook,
                modifier = Modifier.weight(1f),
            )
            return@Column
        }
        val listState = rememberLazyListState()
        hideOnScroll.BindListTop(listState)
        val more = loaded?.more == true
        LoadWhenNearEnd(
            listState = listState,
            enabled = loaded != null && !loaded.more && loaded.page.cursor() != null,
            onLoad = viewModel::loadMore,
        )
        LazyColumn(
            state = listState,
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .nestedScroll(hideOnScroll.connection),
        ) {
            if (loading || more) {
                item(key = "status-loading") { SpinnerStatus() }
            }
            failed?.let { error ->
                item(key = "status-error") {
                    ErrorStatus(message = error.error, onRetry = viewModel::retry)
                }
            }
            when (page) {
                is SearchPage.Accounts -> items(page.items, key = { "account-${it.username}" }) { account ->
                    AccountHitRow(account) { onOpenProfile(account.username, account.username) }
                }
                is SearchPage.Books -> items(page.items, key = { "book-${it.id}" }) { book ->
                    BookPost(
                        book = book,
                        cover = library.coverFile(book.id),
                        showOwner = hosted && !book.mine,
                        onOpen = { onOpenBook(book.id) },
                        onDelete = null,
                        onShare = null,
                    )
                    HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
                }
                is SearchPage.Posts -> items(page.items, key = { "post-${it.book.id}-${it.post.id}" }) { hit ->
                    SearchPost(
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
                null -> Unit
            }
            if (showNoMatches(search)) {
                item(key = "empty") { SearchEmpty() }
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

private fun SearchPage.isBlank(): Boolean = when (this) {
    is SearchPage.Accounts -> items.isEmpty()
    is SearchPage.Books -> items.isEmpty()
    is SearchPage.Posts -> items.isEmpty()
}

@Composable
internal fun SearchField(
    query: String,
    onQueryChange: (String) -> Unit,
    hosted: Boolean,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
) {
    val shape = RoundedCornerShape(999.dp)
    val interaction = remember { MutableInteractionSource() }
    val focused by interaction.collectIsFocusedAsState()
    val mute = MaterialTheme.colorScheme.onSurfaceVariant
    val label = if (hosted) "Search accounts, books, and posts" else "Search books and posts"
    BasicTextField(
        value = query,
        onValueChange = onQueryChange,
        singleLine = true,
        textStyle = MaterialTheme.typography.bodyMedium.merge(
            TextStyle(color = MaterialTheme.colorScheme.onBackground),
        ),
        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
        interactionSource = interaction,
        modifier = modifier
            .height(38.dp)
            .border(
                width = 1.dp,
                color = if (focused) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline,
                shape = shape,
            )
            .background(MaterialTheme.colorScheme.surfaceVariant, shape)
            .then(if (focusRequester == null) Modifier else Modifier.focusRequester(focusRequester))
            .semantics { contentDescription = label },
        decorationBox = { inner ->
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(38.dp)
                    .padding(start = 12.dp, end = if (query.isEmpty()) 12.dp else 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Search,
                    contentDescription = null,
                    tint = mute,
                    modifier = Modifier.size(18.dp),
                )
                Spacer(Modifier.width(8.dp))
                Box(Modifier.weight(1f)) {
                    if (query.isEmpty()) {
                        Text(
                            text = "Search",
                            style = MaterialTheme.typography.bodyMedium,
                            color = mute,
                        )
                    }
                    inner()
                }
                if (query.isNotEmpty()) {
                    Box(
                        modifier = Modifier
                            .size(28.dp)
                            .clickable(role = Role.Button, onClick = { onQueryChange("") })
                            .semantics { contentDescription = "Clear search" },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Close,
                            contentDescription = null,
                            tint = mute,
                            modifier = Modifier.size(16.dp),
                        )
                    }
                }
            }
        },
    )
}

@Composable
internal fun AccountHitRow(account: AccountHit, onOpen: () -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(role = Role.Button, onClick = onOpen)
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Avatar(name = account.username, size = 40.dp)
            Spacer(Modifier.width(12.dp))
            Column {
                Text(
                    text = account.username,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    text = "@${account.username}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
    }
}

@Composable
private fun SearchPost(
    hit: PostHitRecord,
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
internal fun SearchEmpty() {
    Text(
        text = "No matches",
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
