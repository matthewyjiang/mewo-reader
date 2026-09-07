package com.mewo.reader.ui.reader

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Favorite
import androidx.compose.material.icons.outlined.FavoriteBorder
import androidx.compose.material.icons.outlined.IosShare
import androidx.compose.material.icons.outlined.Repeat
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.PostKind
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.theme.LocalMewoColors
import java.io.File

@Composable
fun TimelinePost(
    post: FeedPost,
    author: String,
    handle: String,
    cover: File?,
    liked: Boolean,
    onLike: () -> Unit,
    onRepost: () -> Unit,
    onShare: () -> Unit,
    onOpen: (() -> Unit)? = null,
) {
    val isHeading = post.kind == PostKind.Heading
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .then(
                if (onOpen != null) Modifier.clickable(role = Role.Button, onClick = onOpen)
                else Modifier,
            )
            .padding(start = 16.dp, end = 12.dp, top = 12.dp),
    ) {
        Avatar(name = author, coverFile = cover, size = 40.dp)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = author,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                Spacer(Modifier.width(6.dp))
                Text(
                    text = "@$handle · ${post.chapter}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
            }
            Spacer(Modifier.height(4.dp))
            Text(
                text = post.text,
                style = if (isHeading) {
                    MaterialTheme.typography.titleMedium
                } else {
                    MaterialTheme.typography.bodyLarge
                },
                fontWeight = if (isHeading) FontWeight.Bold else FontWeight.Normal,
            )
            Spacer(Modifier.height(4.dp))
            ActionRow(
                liked = liked,
                onLike = onLike,
                onRepost = onRepost,
                onShare = onShare,
            )
            Spacer(Modifier.height(4.dp))
        }
    }
    HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
}

@Composable
private fun ActionRow(
    liked: Boolean,
    onLike: () -> Unit,
    onRepost: () -> Unit,
    onShare: () -> Unit,
) {
    val mute = MaterialTheme.colorScheme.onSurfaceVariant
    val likeColor = LocalMewoColors.current.like
    val likeScale by animateFloatAsState(if (liked) 1.18f else 1f, label = "like")
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ActionIcon(
            icon = Icons.Outlined.Repeat,
            tint = mute,
            label = "Quote this line",
            onClick = onRepost,
        )
        ActionIcon(
            icon = if (liked) Icons.Outlined.Favorite else Icons.Outlined.FavoriteBorder,
            tint = if (liked) likeColor else mute,
            label = if (liked) "Unlike" else "Like",
            onClick = onLike,
            modifier = Modifier.scale(likeScale),
        )
        ActionIcon(
            icon = Icons.Outlined.IosShare,
            tint = mute,
            label = "Share",
            onClick = onShare,
        )
    }
}

@Composable
private fun ActionIcon(
    icon: ImageVector,
    tint: Color,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    IconButton(
        onClick = onClick,
        modifier = modifier
            .size(40.dp)
            .semantics {
                contentDescription = label
                role = Role.Button
            },
    ) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(18.dp))
    }
}
