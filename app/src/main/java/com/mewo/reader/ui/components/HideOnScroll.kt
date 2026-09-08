package com.mewo.reader.ui.components

import android.content.Context
import android.view.ViewConfiguration
import android.view.accessibility.AccessibilityManager
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize

/**
 * Shared hide-on-scroll chrome for the top bar and tab bar.
 *
 * Threshold is the system touch slop, so a jitter below what Android
 * already treats as a drag cannot flip the bars. Duration is Material
 * medium-short (200ms). TalkBack keeps the bars on.
 */
class HideOnScrollState(
    private val touchSlopPx: Float,
    private val enabled: Boolean,
) {
    var visible by mutableStateOf(true)
        private set

    private var accumulated = 0f

    fun show() {
        visible = true
        accumulated = 0f
    }

    val connection = object : NestedScrollConnection {
        override fun onPostScroll(
            consumed: Offset,
            available: Offset,
            source: NestedScrollSource,
        ): Offset {
            if (!enabled) return Offset.Zero
            val delta = if (consumed.y != 0f) consumed.y else available.y
            if (delta == 0f) return Offset.Zero
            accumulated += delta
            when {
                accumulated <= -touchSlopPx -> {
                    visible = false
                    accumulated = 0f
                }
                accumulated >= touchSlopPx -> {
                    visible = true
                    accumulated = 0f
                }
            }
            return Offset.Zero
        }
    }
}

@Composable
fun rememberHideOnScrollState(): HideOnScrollState {
    val context = LocalContext.current
    val touchSlop = remember {
        ViewConfiguration.get(context).scaledTouchSlop.toFloat()
    }
    val explore = remember {
        val am = context.getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
        am.isTouchExplorationEnabled
    }
    return remember(touchSlop, explore) {
        HideOnScrollState(touchSlopPx = touchSlop, enabled = !explore)
    }
}

/** Restore chrome when the feed cannot scroll (empty, loading, or no hits). */
@Composable
fun HideOnScrollState.ShowWhenIdle(idle: Boolean) {
    LaunchedEffect(this, idle) {
        if (idle) show()
    }
}

/** Bring chrome back when the feed is pinned at the top. */
@Composable
fun HideOnScrollState.BindListTop(listState: LazyListState) {
    LaunchedEffect(this, listState) {
        snapshotFlow {
            listState.firstVisibleItemIndex == 0 &&
                listState.firstVisibleItemScrollOffset == 0
        }.collect { atTop -> if (atTop) show() }
    }
}

internal object ChromeMotion {
    const val DurationMs = 200
    private val sizeSpec = tween<IntSize>(DurationMs, easing = FastOutSlowInEasing)
    private val offsetSpec = tween<IntOffset>(DurationMs, easing = FastOutSlowInEasing)

    fun topEnter() = slideInVertically(offsetSpec) { -it } +
        expandVertically(sizeSpec, expandFrom = Alignment.Top)

    fun topExit() = slideOutVertically(offsetSpec) { -it } +
        shrinkVertically(sizeSpec, shrinkTowards = Alignment.Top)

    fun bottomEnter() = slideInVertically(offsetSpec) { it } +
        expandVertically(sizeSpec, expandFrom = Alignment.Bottom)

    fun bottomExit() = slideOutVertically(offsetSpec) { it } +
        shrinkVertically(sizeSpec, shrinkTowards = Alignment.Bottom)
}
