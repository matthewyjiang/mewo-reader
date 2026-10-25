package com.mewo.reader.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.HostedClient
import com.mewo.reader.data.OfflineSummary
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalHostedAuth
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** null (still loading, or the call failed) keeps the create link. */
internal fun showCreateAccount(allowSignup: Boolean?): Boolean = allowSignup != false

private val usernamePattern = Regex("^[A-Za-z0-9_]{3,32}$")
private const val USERNAME_RULE = "3–32 characters: letters, digits, underscore."
private const val PASSWORD_RULE = "At least 8 characters."

private enum class AuthMode { SignIn, Create }

/** Local vs hosted library. Lives on the Library settings page. */
@Composable
fun LibrarySettings(
    modifier: Modifier = Modifier,
    startOnCreate: Boolean = false,
    signupsOpen: Boolean? = null,
    offlineSummary: OfflineSummary = OfflineSummary(downloaded = 0, total = 1, running = false),
    onDownloadAll: () -> Unit = {},
    unsentReplies: Int = 0,
    onDiscardOutbox: suspend () -> Unit = {},
) {
    val backend = LocalBackend.current
    Column(modifier = modifier.fillMaxWidth()) {
        Column(modifier = Modifier.selectableGroup()) {
            BackendRow(
                title = "On this phone",
                selected = backend.kind == BackendKind.Local,
                onSelect = { backend.setKind(BackendKind.Local) },
            )
            BackendRow(
                title = "Hosted server",
                selected = backend.kind == BackendKind.Hosted,
                onSelect = { backend.setKind(BackendKind.Hosted) },
            )
        }
        if (backend.kind == BackendKind.Hosted) {
            HostedAccountForm(
                startOnCreate = startOnCreate,
                signupsOpen = signupsOpen,
                offlineSummary = offlineSummary,
                onDownloadAll = onDownloadAll,
                unsentReplies = unsentReplies,
                onDiscardOutbox = onDiscardOutbox,
            )
        }
    }
}

@Composable
private fun HostedAccountForm(
    startOnCreate: Boolean,
    signupsOpen: Boolean?,
    offlineSummary: OfflineSummary,
    onDownloadAll: () -> Unit,
    unsentReplies: Int,
    onDiscardOutbox: suspend () -> Unit,
) {
    val hosted = LocalHostedAuth.current
    val scope = rememberCoroutineScope()
    var mode by remember { mutableStateOf(if (startOnCreate) AuthMode.Create else AuthMode.SignIn) }
    // A caller-supplied value (screenshots) skips the network. Otherwise null until the fetch.
    var allowSignup by remember { mutableStateOf(signupsOpen) }
    val baseUrl = hosted.session.baseUrl
    val client = remember { HostedClient() }
    if (signupsOpen == null && !hosted.session.signedIn) {
        LaunchedEffect(baseUrl) {
            allowSignup = null
            val url = hosted.session.normalizedUrl() ?: return@LaunchedEffect
            delay(400)
            allowSignup = try {
                withContext(Dispatchers.IO) { client.instance(url).allowSignup }
            } catch (err: CancellationException) {
                throw err
            } catch (_: Exception) {
                null
            }
        }
    }
    LaunchedEffect(allowSignup) {
        if (allowSignup == false && mode == AuthMode.Create) mode = AuthMode.SignIn
    }
    var username by remember { mutableStateOf(hosted.session.username.orEmpty()) }
    var password by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var discardCount by remember { mutableStateOf(0) }
    val usernameError = usernameError(username)
    val passwordError = passwordError(password)
    val ready = !busy &&
        usernamePattern.matches(username.trim()) &&
        password.length >= 8 &&
        hosted.session.normalizedUrl() != null

    fun runAuth(block: suspend () -> Unit) {
        scope.launch {
            busy = true
            error = null
            runCatching { block() }
                .onSuccess { password = "" }
                .onFailure { err ->
                    error = err.message ?: "Could not reach the server."
                }
            busy = false
        }
    }

    Column(modifier = Modifier.fillMaxWidth()) {
        if (hosted.session.signedIn) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 52.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                Text(
                    text = "@${hosted.session.username}",
                    style = MaterialTheme.typography.titleMedium,
                )
            }
            DownloadAllRow(summary = offlineSummary, onClick = onDownloadAll)
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 52.dp)
                    .clickable(enabled = !busy, role = Role.Button) {
                        if (unsentReplies > 0) {
                            discardCount = unsentReplies
                        } else {
                            runAuth { hosted.signOut() }
                        }
                    },
                contentAlignment = Alignment.CenterStart,
            ) {
                Text(
                    text = "Sign out",
                    color = MaterialTheme.colorScheme.primary,
                    style = MaterialTheme.typography.titleMedium,
                )
            }
        } else {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 52.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                Text(
                    text = if (mode == AuthMode.SignIn) "Sign in" else "Create account",
                    style = MaterialTheme.typography.titleLarge,
                )
            }
            OutlinedTextField(
                value = hosted.session.baseUrl,
                onValueChange = {
                    hosted.setBaseUrl(it)
                    error = null
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy,
                singleLine = true,
                label = { Text("Server URL") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                textStyle = MaterialTheme.typography.bodyMedium,
            )
            OutlinedTextField(
                value = username,
                onValueChange = {
                    username = it
                    error = null
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy,
                singleLine = true,
                isError = usernameError != null,
                supportingText = usernameError?.let { message -> { Text(message) } },
                label = { Text("Username") },
                textStyle = MaterialTheme.typography.bodyMedium,
            )
            OutlinedTextField(
                value = password,
                onValueChange = {
                    password = it
                    error = null
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy,
                singleLine = true,
                isError = passwordError != null,
                supportingText = passwordError?.let { message -> { Text(message) } },
                label = { Text("Password") },
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                textStyle = MaterialTheme.typography.bodyMedium,
            )
            error?.let { message ->
                Text(
                    text = message,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            FormButton(
                modifier = Modifier.padding(top = 12.dp),
                text = if (busy) {
                    "Connecting…"
                } else if (mode == AuthMode.SignIn) {
                    "Sign in"
                } else {
                    "Create account"
                },
                enabled = ready,
                onClick = {
                    runAuth {
                        if (mode == AuthMode.SignIn) {
                            hosted.signIn(username, password)
                        } else {
                            hosted.createAccount(username, password)
                        }
                    }
                },
            )
            if (mode == AuthMode.Create || showCreateAccount(allowSignup)) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 40.dp)
                        .clickable(enabled = !busy, role = Role.Button) {
                            mode = if (mode == AuthMode.SignIn) AuthMode.Create else AuthMode.SignIn
                            error = null
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = if (mode == AuthMode.SignIn) "Create an account" else "Sign in instead",
                        color = MaterialTheme.colorScheme.primary,
                        style = MaterialTheme.typography.bodyLarge,
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
    }
    if (discardCount > 0) {
        DiscardUnsentDialog(
            count = discardCount,
            onConfirm = {
                discardCount = 0
                runAuth {
                    discardThenSignOut(onDiscardOutbox, hosted.signOut)
                }
            },
            onDismiss = { discardCount = 0 },
        )
    }
}

/**
 * The queue is cleared before the landed reply is deleted. Sign-out still finishes
 * when that delete fails, and the failure is raised for the caller to show.
 */
internal suspend fun discardThenSignOut(
    discard: suspend () -> Unit,
    signOut: suspend () -> Unit,
) {
    val failure = try {
        discard()
        null
    } catch (err: CancellationException) {
        throw err
    } catch (err: Exception) {
        err
    }
    signOut()
    failure?.let { throw it }
}

/** Title for the sign-out confirm. One reply stays singular. */
internal fun discardUnsentTitle(count: Int): String {
    val noun = if (count == 1) "reply" else "replies"
    return "Discard $count unsent $noun?"
}

/** Same alert shape as [com.mewo.reader.ui.library.RemoveBookDialog]. */
@Composable
internal fun DiscardUnsentDialog(
    count: Int,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = MaterialTheme.colorScheme.surface,
        titleContentColor = MaterialTheme.colorScheme.onBackground,
        textContentColor = MaterialTheme.colorScheme.onSurfaceVariant,
        title = {
            Text(
                text = discardUnsentTitle(count),
                style = MaterialTheme.typography.titleLarge,
            )
        },
        confirmButton = {
            TextButton(onClick = onConfirm) {
                Text("Discard", color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel", color = MaterialTheme.colorScheme.primary)
            }
        },
    )
}

@Composable
internal fun DownloadAllRow(
    summary: OfflineSummary,
    onClick: () -> Unit,
) {
    val enabled = downloadAllEnabled(summary)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .then(
                if (enabled) {
                    Modifier.clickable(role = Role.Button, onClick = onClick)
                } else {
                    Modifier
                },
            ),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(
            text = downloadAllLabel(summary),
            color = if (enabled) MaterialTheme.colorScheme.primary else Color.Unspecified,
            style = MaterialTheme.typography.titleMedium,
        )
    }
}

internal fun downloadAllLabel(summary: OfflineSummary): String = when {
    summary.running -> "Downloading ${summary.downloaded} of ${summary.total}"
    summary.downloaded < summary.total -> "Download all books"
    else -> "All books downloaded"
}

internal fun downloadAllEnabled(summary: OfflineSummary): Boolean =
    summary.running || summary.downloaded < summary.total

@Composable
private fun FormButton(
    text: String,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val accent = MaterialTheme.colorScheme.primary
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(40.dp)
            .clip(RoundedCornerShape(999.dp))
            .background(if (enabled) accent else accent.copy(alpha = 0.35f))
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = text,
            color = MaterialTheme.colorScheme.onPrimary,
            style = MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.Bold,
        )
    }
}

@Composable
private fun BackendRow(
    title: String,
    selected: Boolean,
    onSelect: () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .selectable(
                selected = selected,
                onClick = onSelect,
                role = Role.RadioButton,
            ),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium,
            color = if (selected) {
                MaterialTheme.colorScheme.onBackground
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
        )
    }
}

private fun usernameError(value: String): String? {
    val trimmed = value.trim()
    if (trimmed.isEmpty() || usernamePattern.matches(trimmed)) return null
    return USERNAME_RULE
}

private fun passwordError(value: String): String? {
    if (value.isEmpty() || value.length >= 8) return null
    return PASSWORD_RULE
}
