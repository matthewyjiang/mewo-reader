package com.mewo.reader.ui.reader

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.MoreHoriz
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.PostKind
import com.mewo.reader.ui.LocalLibraryStore
import com.mewo.reader.ui.commenterIdentity
import com.mewo.reader.ui.currentReaderIdentity
import com.mewo.reader.ui.components.Avatar
import com.mewo.reader.ui.theme.LocalReaderFont
import kotlinx.coroutines.launch
import java.io.File

data class CommentTarget(
    val book: BookRecord,
    val post: FeedPost,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CommentSheet(
    target: CommentTarget,
    publicNotes: Boolean,
    onThreadChanged: (postId: String, comments: List<PostComment>) -> Unit,
    onDismiss: () -> Unit,
    onOpenProfile: ((handle: String, name: String) -> Unit)? = null,
) {
    val store = LocalLibraryStore.current
    val me = currentReaderIdentity()
    val meName = me.name
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    var comments by remember { mutableStateOf<List<PostComment>>(emptyList()) }
    var draft by remember { mutableStateOf("") }
    var sending by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var focused by remember { mutableStateOf(false) }

    fun publishThread(next: List<PostComment>) {
        comments = next
        onThreadChanged(target.post.id, next)
    }

    fun postReply() {
        val text = draft.trim()
        if (text.isEmpty() || sending) return
        sending = true
        scope.launch {
            runCatching { store.addComment(target.book.id, target.post.id, text) }
                .onSuccess { next ->
                    draft = ""
                    focused = false
                    publishThread(next)
                }
                .onFailure { err ->
                    error = err.message ?: "Could not post that reply."
                }
            sending = false
        }
    }

    LaunchedEffect(target.book.id, target.post.id) {
        error = null
        runCatching { store.comments(target.book.id, target.post.id) }
            .onSuccess { publishThread(it) }
            .onFailure { err ->
                error = err.message ?: "Could not load replies."
            }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = MaterialTheme.colorScheme.background,
        contentColor = MaterialTheme.colorScheme.onBackground,
        dragHandle = null,
        tonalElevation = 0.dp,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .windowInsetsPadding(
                    WindowInsets.safeDrawing.only(
                        WindowInsetsSides.Horizontal + WindowInsetsSides.Top,
                    ),
                )
                .navigationBarsPadding()
                .imePadding(),
        ) {
            ComposerBar(
                canReply = draft.isNotBlank() && !sending,
                onClose = onDismiss,
                onReply = ::postReply,
            )
            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f),
            ) {
                item {
                    ThreadPost(
                        name = target.book.author,
                        handle = target.book.handle,
                        meta = target.post.chapter,
                        text = target.post.text,
                        heading = target.post.kind == PostKind.Heading,
                        cover = store.coverFile(target.book.id),
                        connectDown = true,
                    )
                }
                items(comments, key = { it.id }) { comment ->
                    val identity = commenterIdentity(comment)
                    ThreadPost(
                        name = identity.name,
                        handle = identity.handle,
                        meta = relativeTime(comment.createdAt),
                        text = comment.text,
                        heading = false,
                        cover = null,
                        connectDown = true,
                        onOpenProfile = onOpenProfile,
                        onDelete = if (comment.mine) {
                            {
                                scope.launch {
                                    runCatching {
                                        store.deleteComment(
                                            target.book.id,
                                            target.post.id,
                                            comment.id,
                                        )
                                    }.onSuccess { publishThread(it) }
                                        .onFailure { err ->
                                            error = err.message ?: "Could not delete that reply."
                                        }
                                }
                            }
                        } else {
                            null
                        },
                    )
                }
                item {
                    ComposePost(
                        name = meName,
                        draft = draft,
                        focused = focused,
                        replyHandle = target.book.handle,
                        publicNotes = publicNotes,
                        enabled = !sending,
                        error = error,
                        onDraftChange = {
                            draft = it
                            error = null
                        },
                        onFocus = { focused = it },
                        onOpenProfile = onOpenProfile?.let { open ->
                            { open(me.handle, me.name) }
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun ComposerBar(
    canReply: Boolean,
    onClose: () -> Unit,
    onReply: () -> Unit,
) {
    Column(Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(56.dp)
                .padding(end = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onClose) {
                Icon(
                    Icons.Outlined.Close,
                    contentDescription = "Close",
                    tint = MaterialTheme.colorScheme.onBackground,
                )
            }
            Spacer(Modifier.weight(1f))
            ReplyPill(enabled = canReply, onClick = onReply)
        }
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
    }
}

@Composable
private fun ReplyPill(
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val accent = MaterialTheme.colorScheme.primary
    Text(
        text = "Reply",
        color = MaterialTheme.colorScheme.onPrimary,
        style = MaterialTheme.typography.labelLarge,
        fontWeight = FontWeight.Bold,
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(if (enabled) accent else accent.copy(alpha = 0.35f))
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp)
            .semantics { contentDescription = "Reply" },
    )
}

@Composable
fun ThreadPost(
    name: String,
    handle: String,
    meta: String,
    text: String,
    heading: Boolean,
    cover: File?,
    connectDown: Boolean,
    onOpenProfile: ((handle: String, name: String) -> Unit)? = null,
    onDelete: (() -> Unit)? = null,
) {
    val readerType = LocalReaderFont.current.scale
    var menu by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .padding(start = 16.dp, end = 4.dp, top = 12.dp),
    ) {
        Column(
            modifier = Modifier
                .width(40.dp)
                .fillMaxHeight(),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Avatar(
                name = name,
                coverFile = cover,
                size = 40.dp,
                modifier = if (onOpenProfile != null) {
                    Modifier.clickable(role = Role.Button) { onOpenProfile(handle, name) }
                } else {
                    Modifier
                },
            )
            if (connectDown) {
                Box(
                    modifier = Modifier
                        .padding(top = 4.dp)
                        .width(2.dp)
                        .weight(1f)
                        .background(MaterialTheme.colorScheme.outline),
                )
            }
        }
        Spacer(Modifier.width(12.dp))
        Column(
            modifier = Modifier
                .weight(1f)
                .padding(bottom = 12.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = name,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier
                        .weight(1f, fill = false)
                        .then(
                            if (onOpenProfile != null) {
                                Modifier.clickable(role = Role.Button) {
                                    onOpenProfile(handle, name)
                                }
                            } else {
                                Modifier
                            },
                        ),
                )
                Spacer(Modifier.width(4.dp))
                Text(
                    text = "@$handle · $meta",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier
                        .weight(1f)
                        .then(
                            if (onOpenProfile != null) {
                                Modifier.clickable(role = Role.Button) {
                                    onOpenProfile(handle, name)
                                }
                            } else {
                                Modifier
                            },
                        ),
                )
                if (onDelete != null) {
                    Box {
                        IconButton(
                            onClick = { menu = true },
                            modifier = Modifier.size(40.dp),
                        ) {
                            Icon(
                                Icons.Outlined.MoreHoriz,
                                contentDescription = "More",
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.size(18.dp),
                            )
                        }
                        DropdownMenu(
                            expanded = menu,
                            onDismissRequest = { menu = false },
                            containerColor = MaterialTheme.colorScheme.surface,
                        ) {
                            DropdownMenuItem(
                                text = {
                                    Text(
                                        "Delete",
                                        color = MaterialTheme.colorScheme.error,
                                        style = MaterialTheme.typography.bodyLarge,
                                    )
                                },
                                onClick = {
                                    menu = false
                                    onDelete()
                                },
                            )
                        }
                    }
                }
            }
            Spacer(Modifier.height(2.dp))
            Text(
                text = text,
                style = if (heading) {
                    readerType.headingStyle(MaterialTheme.typography.titleMedium)
                } else {
                    readerType.bodyStyle(MaterialTheme.typography.bodyLarge)
                },
            )
        }
    }
}

@Composable
private fun ComposePost(
    name: String,
    draft: String,
    focused: Boolean,
    replyHandle: String,
    publicNotes: Boolean,
    enabled: Boolean,
    error: String?,
    onDraftChange: (String) -> Unit,
    onFocus: (Boolean) -> Unit,
    onOpenProfile: (() -> Unit)? = null,
) {
    val showHint = focused || draft.isNotEmpty()
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 12.dp),
    ) {
        Avatar(
            name = name,
            size = 40.dp,
            modifier = if (onOpenProfile != null) {
                Modifier.clickable(role = Role.Button, onClick = onOpenProfile)
            } else {
                Modifier
            },
        )
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            if (showHint) {
                Text(
                    text = "Replying to @$replyHandle",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.padding(bottom = 4.dp),
                )
            }
            BasicTextField(
                value = draft,
                onValueChange = onDraftChange,
                enabled = enabled,
                textStyle = MaterialTheme.typography.bodyLarge.merge(
                    TextStyle(color = MaterialTheme.colorScheme.onBackground),
                ),
                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 40.dp)
                    .onFocusChanged { onFocus(it.isFocused) }
                    .semantics {
                        contentDescription = if (publicNotes) {
                            "Reply, public on this server"
                        } else {
                            "Reply"
                        }
                    },
                decorationBox = { inner ->
                    if (draft.isEmpty()) {
                        Text(
                            text = "Post your reply",
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    inner()
                },
            )
            error?.let { message ->
                Spacer(Modifier.height(6.dp))
                Text(
                    text = message,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.error,
                )
            }
        }
    }
}

fun relativeTime(createdAt: Long, now: Long = System.currentTimeMillis()): String {
    val delta = (now - createdAt).coerceAtLeast(0)
    val minutes = delta / 60_000
    if (minutes < 1) return "now"
    if (minutes < 60) return "${minutes}m"
    val hours = minutes / 60
    if (hours < 24) return "${hours}h"
    val days = hours / 24
    return "${days}d"
}
