package com.mewo.reader.ui.components

import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.snapshotFlow
import kotlinx.coroutines.flow.distinctUntilChanged

@Composable
fun LoadWhenNearEnd(
    listState: LazyListState,
    enabled: Boolean,
    onLoad: () -> Unit,
) {
    LaunchedEffect(listState, enabled) {
        if (!enabled) return@LaunchedEffect
        snapshotFlow {
            val info = listState.layoutInfo
            val last = info.visibleItemsInfo.lastOrNull()?.index ?: -1
            info.totalItemsCount > 0 && last >= info.totalItemsCount - 2
        }
            .distinctUntilChanged()
            .collect { nearEnd -> if (nearEnd) onLoad() }
    }
}
