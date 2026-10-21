package com.mewo.reader.ui.catalog

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.MenuBook
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogCovers
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.ui.theme.LocalMewoColors
import java.io.File

@Composable
fun CatalogRow(
    work: CatalogWork,
    request: BookRequest?,
    onOpen: () -> Unit,
    onAct: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val action = rowAction(request, work.onServer)
    val label = badge(work, request)
    val tone = label?.let { badgeTone(it) } ?: BadgeTone.Progress
    val icon = rowIcon(action)
    val actionName = when (action) {
        RowAction.Request -> "Request"
        RowAction.Again -> "Request again"
        RowAction.Pending -> request?.let { stateLabel(it) } ?: "Searching"
        RowAction.Open -> openLabel(work, request)
        RowAction.Retry -> "Retry"
    }
    Column(modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(
                modifier = Modifier
                    .weight(1f)
                    .clickable(role = Role.Button, onClick = onOpen)
                    .padding(start = 16.dp, top = 12.dp, bottom = 12.dp, end = 4.dp)
                    .semantics { contentDescription = catalogRowLabel(work, request) },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                CatalogCover(title = work.title, coverId = work.coverId, width = 40.dp, height = 60.dp)
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(
                        text = work.title,
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Bold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    val meta = buildString {
                        append(work.author)
                        if (work.year != null) {
                            if (isNotEmpty()) append(" · ")
                            append(work.year)
                        }
                    }
                    if (meta.isNotEmpty()) {
                        Text(
                            text = meta,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    if (label != null) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(
                                Modifier
                                    .size(8.dp)
                                    .clip(CircleShape)
                                    .background(toneColor(tone)),
                            )
                            Spacer(Modifier.width(6.dp))
                            Text(
                                text = label,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurface,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
            }
            if (icon != null) {
                Box(
                    modifier = Modifier
                        .padding(end = 4.dp)
                        .size(44.dp)
                        .clickable(role = Role.Button, onClick = onAct)
                        .semantics { contentDescription = actionName },
                    contentAlignment = Alignment.Center,
                ) {
                    Box(
                        modifier = Modifier
                            .size(32.dp)
                            .border(1.5.dp, MaterialTheme.colorScheme.primary, CircleShape),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = icon,
                            contentDescription = null,
                            modifier = Modifier.size(16.dp),
                            tint = MaterialTheme.colorScheme.primary,
                        )
                    }
                }
            } else {
                Spacer(Modifier.width(12.dp))
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
    }
}

@Composable
internal fun CatalogCover(
    title: String,
    coverId: Int?,
    width: Dp,
    height: Dp,
) {
    val context = LocalContext.current
    var file by remember(coverId) { mutableStateOf<File?>(null) }
    LaunchedEffect(coverId) {
        val id = coverId
        val cacheDir: File? = context.cacheDir
        if (id == null || cacheDir == null) {
            file = null
            return@LaunchedEffect
        }
        val covers = CatalogCovers(cacheDir)
        file = covers.cached(id) ?: covers.load(id)
    }
    val bitmap = remember(file?.path, file?.lastModified()) { file.decodeCover() }
    val shape = RoundedCornerShape(3.dp)
    Box(
        modifier = Modifier
            .size(width, height)
            .clip(shape)
            .border(0.6.dp, MaterialTheme.colorScheme.outline, shape)
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        if (bitmap != null) {
            Image(
                bitmap = bitmap,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
            )
        } else {
            Text(
                text = title.trim().firstOrNull()?.uppercaseChar()?.toString() ?: "?",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                fontWeight = FontWeight.Bold,
                style = MaterialTheme.typography.titleMedium,
            )
        }
    }
}

@Composable
private fun toneColor(tone: BadgeTone) = when (tone) {
    BadgeTone.Shelf -> LocalMewoColors.current.repost
    BadgeTone.Failed -> MaterialTheme.colorScheme.error
    BadgeTone.Server -> MaterialTheme.colorScheme.primary
    BadgeTone.Progress -> MaterialTheme.colorScheme.onSurfaceVariant
}

/** A live row already says its status, so it has no trailing control. */
private fun rowIcon(action: RowAction): ImageVector? = when (action) {
    RowAction.Request, RowAction.Again -> Icons.Outlined.Add
    RowAction.Open -> Icons.AutoMirrored.Outlined.MenuBook
    RowAction.Retry -> Icons.Outlined.Refresh
    RowAction.Pending -> null
}

private fun File?.decodeCover() = this
    ?.takeIf { it.exists() && it.length() > 0L }
    ?.let { BitmapFactory.decodeFile(it.absolutePath)?.asImageBitmap() }
