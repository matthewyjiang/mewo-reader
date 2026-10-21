package com.mewo.reader.ui.catalog

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.BorderStroke
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.ui.components.BindListTop
import com.mewo.reader.ui.components.ErrorStatus
import com.mewo.reader.ui.components.HideOnScrollState
import com.mewo.reader.ui.components.LoadWhenNearEnd
import com.mewo.reader.ui.components.SpinnerStatus

@Composable
fun CatalogResults(
    state: CatalogListState,
    query: String,
    hideOnScroll: HideOnScrollState,
    onOpen: (CatalogWork) -> Unit,
    onAct: (CatalogWork) -> Unit,
    onRetry: () -> Unit,
    onLoadMore: () -> Unit,
    onDismissNotice: () -> Unit,
    onCloseSheet: () -> Unit,
    onOpenBook: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val phase = state.phase
    val failed = phase as? CatalogPhase.Failed
    val loaded = phase as? CatalogPhase.Loaded
    val page = when (phase) {
        is CatalogPhase.Loaded -> phase.page
        is CatalogPhase.Loading -> phase.previous
        else -> null
    }
    val loading = phase is CatalogPhase.Loading || loaded?.more == true
    val listState = rememberLazyListState()
    hideOnScroll.BindListTop(listState)
    LoadWhenNearEnd(
        listState = listState,
        enabled = loaded != null && !loaded.more && loaded.page.nextPage != null,
        onLoad = onLoadMore,
    )
    Box(modifier.fillMaxSize()) {
        LazyColumn(
            state = listState,
            modifier = Modifier
                .fillMaxSize()
                .nestedScroll(hideOnScroll.connection),
        ) {
            val emptyPage = loaded != null && !loaded.more && page != null && page.items.isEmpty()
            val showTrending = catalogQuery(query) is CatalogQuery.Browse &&
                failed == null &&
                phase !is CatalogPhase.Idle &&
                !emptyPage
            if (showTrending) {
                item(key = "trending") { CatalogHeading("Trending on Open Library") }
            }
            if (failed != null) {
                item(key = "error") { ErrorStatus(message = failed.message, onRetry = onRetry) }
            } else {
                if (loading && page == null) {
                    item(key = "loading") { CatalogSkeleton() }
                }
                if (page != null) {
                    items(page.items, key = { it.workKey }) { work ->
                        CatalogRow(
                            work = work,
                            request = requestOn(state, work),
                            onOpen = { onOpen(work) },
                            onAct = { onAct(work) },
                        )
                    }
                }
                if (loading && page != null) {
                    item(key = "more") { SpinnerStatus() }
                }
                if (loaded != null && !loaded.more && page != null && page.items.isEmpty()) {
                    item(key = "empty") { CatalogEmpty(query) }
                }
            }
        }
        state.notice?.let { message ->
            CatalogNotice(
                message = message,
                onDismiss = onDismissNotice,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(start = 16.dp, end = 16.dp, bottom = 16.dp)
                    .fillMaxWidth(),
            )
        }
        val sheet = state.sheet
        if (sheet != null) {
            val sheetState by sheet.state.collectAsStateWithLifecycle()
            CatalogSheet(
                state = sheetState,
                onRequest = sheet::request,
                onCancel = sheet::cancel,
                onRetry = sheet::retry,
                onReload = sheet::reload,
                onToggleDescription = sheet::toggleDescription,
                onOpen = { id ->
                    onCloseSheet()
                    onOpenBook(id)
                },
                onDismiss = onCloseSheet,
            )
        }
    }
}

@Composable
internal fun CatalogHeading(title: String) {
    Column(Modifier.fillMaxWidth()) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(44.dp)
                .padding(horizontal = 16.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onBackground,
            )
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
    }
}

/** Six placeholder rows. Same box as [CatalogRow], with no trailing control. */
@Composable
internal fun CatalogSkeleton(rows: Int = 6) {
    val bone = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.16f)
    Column(Modifier.fillMaxWidth()) {
        repeat(rows) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(start = 16.dp, top = 12.dp, end = 16.dp, bottom = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    Modifier
                        .size(40.dp, 60.dp)
                        .clip(RoundedCornerShape(3.dp))
                        .background(bone),
                )
                Spacer(Modifier.width(12.dp))
                Column(
                    Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Box(
                        Modifier
                            .fillMaxWidth(0.6f)
                            .height(14.dp)
                            .clip(RoundedCornerShape(4.dp))
                            .background(bone),
                    )
                    Box(
                        Modifier
                            .fillMaxWidth(0.4f)
                            .height(14.dp)
                            .clip(RoundedCornerShape(4.dp))
                            .background(bone),
                    )
                }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
        }
    }
}

/** Hover banner for a row action that failed. The library snackbar stays as it is. */
@Composable
internal fun CatalogNotice(
    message: String,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier,
        shape = RectangleShape,
        color = MaterialTheme.colorScheme.surfaceVariant,
        contentColor = MaterialTheme.colorScheme.onBackground,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        shadowElevation = 0.dp,
    ) {
        Row(
            modifier = Modifier.padding(start = 16.dp, top = 8.dp, end = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onBackground,
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = onDismiss) {
                Icon(
                    imageVector = Icons.Outlined.Close,
                    contentDescription = "Dismiss message",
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
internal fun CatalogEmpty(query: String) {
    val asked = catalogQuery(query)
    val text = if (asked is CatalogQuery.Search) {
        "Nothing in Open Library matches “${asked.q}”. Try the author's name or fewer words."
    } else {
        "No trending books in your languages right now. Search above."
    }
    Text(
        text = text,
        style = MaterialTheme.typography.headlineLarge,
        modifier = Modifier.padding(horizontal = 28.dp, vertical = 48.dp),
    )
}

private fun requestOn(state: CatalogListState, work: CatalogWork): BookRequest? {
    return if (work.workKey in state.overlay) state.overlay[work.workKey] else work.request
}
