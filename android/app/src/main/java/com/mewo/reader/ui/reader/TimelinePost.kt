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
import androidx.compose.material.icons.outlined.ChatBubbleOutline
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.PostKind
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.theme.LocalMewoColors
import com.mewo.reader.ui.theme.LocalReaderFont
import java.io.File

@Composable
fun TimelinePost(
    post: FeedPost,
    author: String,
    handle: String,
    cover: File?,
    liked: Boolean,
    commentCount: Int = 0,
    commented: Boolean = false,
    onComment: () -> Unit,
    onLike: () -> Unit,
    onRepost: () -> Unit,
    onShare: () -> Unit,
    onOpen: (() -> Unit)? = null,
    onChapterClick: (() -> Unit)? = null,
) {
    val isHeading = post.kind == PostKind.Heading
    val readerType = LocalReaderFont.current.scale
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
                    modifier = Modifier
                        .weight(1f)
                        .then(
                            if (onChapterClick != null) {
                                Modifier
                                    .clickable(role = Role.Button, onClick = onChapterClick)
                                    .semantics {
                                        contentDescription = "Chapters, ${post.chapter}"
                                        role = Role.Button
                                    }
                            } else {
                                Modifier
                            },
                        ),
                )
            }
            Spacer(Modifier.height(4.dp))
            Text(
                text = post.text,
                style = if (isHeading) {
                    readerType.headingStyle(MaterialTheme.typography.titleMedium)
                } else {
                    readerType.bodyStyle(MaterialTheme.typography.bodyLarge)
                },
            )
            Spacer(Modifier.height(4.dp))
            PostActions(
                liked = liked,
                commentCount = commentCount,
                commented = commented,
                onComment = onComment,
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
fun PostActions(
    liked: Boolean,
    commentCount: Int,
    commented: Boolean,
    onComment: () -> Unit,
    onLike: () -> Unit,
    onRepost: () -> Unit,
    onShare: () -> Unit,
) {
    val mute = MaterialTheme.colorScheme.onSurfaceVariant
    val likeColor = LocalMewoColors.current.like
    val accent = MaterialTheme.colorScheme.primary
    val likeScale by animateFloatAsState(if (liked) 1.18f else 1f, label = "like")
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ActionIcon(
            icon = Icons.Outlined.ChatBubbleOutline,
            tint = if (commented) accent else mute,
            label = if (commentCount == 0) {
                "Comment on this line"
            } else {
                "Comments, $commentCount"
            },
            count = commentCount,
            onClick = onComment,
        )
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
    count: Int = 0,
) {
    IconButton(
        onClick = onClick,
        modifier = modifier
            .size(width = if (count > 0) 52.dp else 40.dp, height = 40.dp)
            .semantics {
                contentDescription = label
                role = Role.Button
            },
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(18.dp))
            if (count > 0) {
                Spacer(Modifier.width(4.dp))
                Text(
                    text = count.toString(),
                    style = MaterialTheme.typography.labelSmall,
                    color = tint,
                )
            }
        }
    }
}
