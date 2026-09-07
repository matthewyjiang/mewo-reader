package com.mewo.reader.ui.reader

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.FeedPost
import kotlinx.coroutines.flow.first

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChapterSheet(
    title: String,
    headings: List<FeedPost>,
    currentIndex: Int,
    onJump: (FeedPost) -> Unit,
    onDismiss: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val start = currentIndex.coerceIn(0, headings.lastIndex.coerceAtLeast(0))
    val listState = rememberLazyListState(initialFirstVisibleItemIndex = start)
    val screenHeight = LocalConfiguration.current.screenHeightDp.dp
    // Cap the list so it scrolls inside the sheet instead of stretching the sheet.
    LaunchedEffect(start, headings.size) {
        if (headings.isEmpty()) return@LaunchedEffect
        snapshotFlow { listState.layoutInfo.viewportSize.height }
            .first { it > 0 }
        listState.scrollToItem(start)
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = MaterialTheme.colorScheme.surface,
        contentColor = MaterialTheme.colorScheme.onSurface,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(bottom = 28.dp),
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.headlineLarge,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 8.dp),
            )
            if (headings.isEmpty()) {
                Text(
                    text = "This book has no chapter headings.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                )
            } else {
                LazyColumn(
                    state = listState,
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(max = screenHeight * 3 / 4),
                ) {
                    itemsIndexed(headings, key = { _, post -> post.id }) { index, post ->
                        val current = index == currentIndex
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 48.dp)
                                .background(
                                    if (current) MaterialTheme.colorScheme.surfaceVariant
                                    else MaterialTheme.colorScheme.surface,
                                )
                                .clickable(role = Role.Button, onClick = { onJump(post) })
                                .padding(horizontal = 16.dp, vertical = 14.dp),
                            contentAlignment = Alignment.CenterStart,
                        ) {
                            Text(
                                text = post.text,
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = if (current) FontWeight.Bold else FontWeight.Normal,
                                color = if (current) {
                                    MaterialTheme.colorScheme.onBackground
                                } else {
                                    MaterialTheme.colorScheme.onSurfaceVariant
                                },
                            )
                        }
                        HorizontalDivider(
                            color = MaterialTheme.colorScheme.outline,
                            thickness = 0.6.dp,
                        )
                    }
                }
            }
        }
    }
}
