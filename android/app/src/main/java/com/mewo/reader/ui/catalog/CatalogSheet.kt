package com.mewo.reader.ui.catalog

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.material3.ButtonDefaults
import androidx.compose.foundation.BorderStroke

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CatalogSheet(
    state: CatalogSheetState,
    onRequest: () -> Unit,
    onCancel: () -> Unit,
    onRetry: () -> Unit,
    onOpen: (String) -> Unit,
    onReload: () -> Unit,
    onToggleDescription: () -> Unit,
    onDismiss: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = MaterialTheme.colorScheme.background,
        contentColor = MaterialTheme.colorScheme.onBackground,
        dragHandle = null,
        tonalElevation = 0.dp,
    ) {
        CatalogSheetContent(
            state = state,
            onRequest = onRequest,
            onCancel = onCancel,
            onRetry = onRetry,
            onOpen = onOpen,
            onReload = onReload,
            onToggleDescription = onToggleDescription,
            onDismiss = onDismiss,
            modifier = Modifier.fillMaxSize(),
        )
    }
}

@Composable
internal fun CatalogSheetContent(
    state: CatalogSheetState,
    onRequest: () -> Unit,
    onCancel: () -> Unit,
    onRetry: () -> Unit,
    onOpen: (String) -> Unit,
    onReload: () -> Unit,
    onToggleDescription: () -> Unit,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val info = state.detail?.info
    val author = info?.authors?.takeIf { it.isNotEmpty() }?.joinToString(", ") ?: state.work.author
    val year = info?.year ?: state.work.year
    val coverId = info?.coverId ?: state.work.coverId
    Column(
        modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Horizontal + WindowInsetsSides.Top))
            .navigationBarsPadding(),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(56.dp)
                .padding(end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onDismiss) {
                Icon(Icons.Outlined.Close, contentDescription = "Close")
            }
            Text(
                text = state.work.title,
                style = MaterialTheme.typography.titleLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
        Column(
            Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .padding(bottom = 32.dp),
        ) {
            Row(
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                CatalogCover(title = state.work.title, coverId = coverId, width = 72.dp, height = 108.dp)
                Column(Modifier.weight(1f)) {
                    Text(
                        text = state.work.title,
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.Bold,
                    )
                    val meta = buildString {
                        append(author)
                        if (year != null) {
                            if (isNotEmpty()) append(" · ")
                            append(year)
                        }
                    }
                    if (meta.isNotEmpty()) {
                        Text(
                            text = meta,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 4.dp),
                        )
                    }
                }
            }
            StatusBlock(
                state = state,
                onRequest = onRequest,
                onCancel = onCancel,
                onRetry = onRetry,
                onOpen = onOpen,
            )
            AboutBlock(
                state = state,
                onReload = onReload,
                onToggleDescription = onToggleDescription,
                onOpenLibrary = { url ->
                    val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
                    runCatching { context.startActivity(intent) }
                },
            )
        }
    }
}

@Composable
private fun StatusBlock(
    state: CatalogSheetState,
    onRequest: () -> Unit,
    onCancel: () -> Unit,
    onRetry: () -> Unit,
    onOpen: (String) -> Unit,
) {
    val mute = MaterialTheme.colorScheme.onSurfaceVariant
    Column(
        modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        when (val status = state.status) {
            is SheetStatus.Live -> {
                Text(text = status.line, style = MaterialTheme.typography.bodyLarge)
                if (status.progress != null) {
                    LinearProgressIndicator(
                        progress = { status.progress.coerceIn(0, 100) / 100f },
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(4.dp)
                            .clip(CircleShape),
                        color = MaterialTheme.colorScheme.primary,
                        trackColor = MaterialTheme.colorScheme.outline,
                        gapSize = 0.dp,
                        drawStopIndicator = {},
                    )
                }
                OutlineDestructive("Cancel request", enabled = !state.acting, onClick = onCancel)
            }
            is SheetStatus.Failed -> {
                Text(text = status.line, style = MaterialTheme.typography.bodyLarge)
                FilledAction("Retry", enabled = !state.acting && status.capNote == null, onClick = onRetry)
                status.capNote?.let { Note(it, mute) }
            }
            is SheetStatus.Available -> {
                Text(text = status.line, style = MaterialTheme.typography.bodyLarge)
                FilledAction(
                    text = "Open book",
                    enabled = status.bookId != null,
                    onClick = { status.bookId?.let(onOpen) },
                )
            }
            is SheetStatus.Canceled -> {
                FilledAction(
                    text = "Request again",
                    enabled = !state.acting && status.capNote == null,
                    onClick = onRetry,
                )
                status.capNote?.let { Note(it, mute) }
            }
            is SheetStatus.Requestable -> {
                FilledAction(
                    text = "Request",
                    enabled = !state.acting && status.capNote == null,
                    onClick = onRequest,
                )
                status.capNote?.let { Note(it, mute) }
                status.editionNote?.let { Note(it, mute) }
            }
        }
        state.alsoRequested?.let { Note(it, mute) }
        state.notice?.let { message ->
            Text(
                text = message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

@Composable
private fun AboutBlock(
    state: CatalogSheetState,
    onReload: () -> Unit,
    onToggleDescription: () -> Unit,
    onOpenLibrary: (String) -> Unit,
) {
    val about = state.about
    when {
        state.loading && about == null -> {
            Column(
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 28.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Bone()
                Bone(fraction = 0.64f)
                Bone()
            }
        }
        about == null && (state.loadError != null || state.detail != null) -> {
            val message = state.loadError ?: "Couldn't load details from Open Library."
            Text(
                text = message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 28.dp),
            )
            TextButton(onClick = onReload, contentPadding = PaddingValues(horizontal = 16.dp)) {
                Text("Try again", color = MaterialTheme.colorScheme.primary)
            }
        }
        about != null -> {
            Column(
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 28.dp),
                verticalArrangement = Arrangement.spacedBy(24.dp),
            ) {
                about.description?.let { description ->
                    Description(description, state.descriptionExpanded, onToggleDescription)
                }
                if (about.series.isNotEmpty() || about.awards.isNotEmpty()) {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (about.series.isNotEmpty()) {
                            LabeledRow("Series", about.series.joinToString(" · "))
                        }
                        if (about.awards.isNotEmpty()) {
                            LabeledRow("Awards", about.awards.joinToString(" · "))
                        }
                    }
                }
                about.subjects?.let { subjects ->
                    Text(
                        text = subjects,
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                about.stats?.let { stats ->
                    Text(
                        text = stats,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onBackground,
                        maxLines = 1,
                        softWrap = false,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                TextButton(
                    onClick = { onOpenLibrary(about.openLibraryUrl) },
                    contentPadding = PaddingValues(0.dp),
                ) {
                    Text("View on Open Library", color = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(6.dp))
                    Icon(
                        imageVector = Icons.AutoMirrored.Outlined.OpenInNew,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp),
                        tint = MaterialTheme.colorScheme.primary,
                    )
                }
            }
        }
    }
}

@Composable
private fun Description(text: String, expanded: Boolean, onToggle: () -> Unit) {
    var overflow by remember(text) { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            text = text,
            style = MaterialTheme.typography.bodyLarge,
            maxLines = if (expanded) Int.MAX_VALUE else 6,
            overflow = TextOverflow.Ellipsis,
            onTextLayout = { result ->
                if (!expanded) overflow = result.hasVisualOverflow
            },
        )
        if (overflow || expanded) {
            TextButton(onClick = onToggle, contentPadding = PaddingValues(0.dp)) {
                Text(
                    text = if (expanded) "Less" else "More",
                    color = MaterialTheme.colorScheme.primary,
                )
            }
        }
    }
}

@Composable
private fun LabeledRow(label: String, value: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(76.dp),
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyLarge,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun Note(text: String, color: Color) {
    Text(text = text, style = MaterialTheme.typography.bodyMedium, color = color)
}

@Composable
private fun Bone(fraction: Float = 1f) {
    Box(
        Modifier
            .fillMaxWidth(fraction)
            .height(14.dp)
            .clip(RoundedCornerShape(4.dp))
            .background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.16f)),
    )
}

@Composable
private fun FilledAction(text: String, enabled: Boolean, onClick: () -> Unit) {
    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 40.dp),
        shape = CircleShape,
    ) {
        Text(text, color = MaterialTheme.colorScheme.onPrimary)
    }
}

@Composable
private fun OutlineDestructive(text: String, enabled: Boolean, onClick: () -> Unit) {
    OutlinedButton(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 40.dp),
        shape = CircleShape,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        colors = ButtonDefaults.outlinedButtonColors(
            contentColor = MaterialTheme.colorScheme.error,
        ),
    ) {
        Text(text, color = MaterialTheme.colorScheme.error)
    }
}
