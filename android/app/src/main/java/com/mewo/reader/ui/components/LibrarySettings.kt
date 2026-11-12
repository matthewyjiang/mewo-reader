package com.mewo.reader.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.HostedClient
import com.mewo.reader.data.InstanceInfo
import com.mewo.reader.data.Me
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

enum class HostedAuthMode { SignIn, Create, Reset }

/** Trail of the Email row: pending first, then the verified address. */
internal fun emailTrail(me: Me): String = when {
    me.pendingEmail != null -> "Check inbox"
    me.email != null -> me.email
    else -> "Add"
}

/** Local vs hosted library. Lives on the Library settings page. */
@Composable
fun LibrarySettings(
    modifier: Modifier = Modifier,
    startMode: HostedAuthMode = HostedAuthMode.SignIn,
    instance: InstanceInfo? = null,
    me: Me? = null,
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
                startMode = startMode,
                instance = instance,
                me = me,
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
    startMode: HostedAuthMode,
    instance: InstanceInfo?,
    me: Me?,
    offlineSummary: OfflineSummary,
    onDownloadAll: () -> Unit,
    unsentReplies: Int,
    onDiscardOutbox: suspend () -> Unit,
) {
    val hosted = LocalHostedAuth.current
    val scope = rememberCoroutineScope()
    var mode by remember { mutableStateOf(startMode) }
    // A caller-supplied value (screenshots) skips the network. Otherwise null until the fetch.
    var info by remember { mutableStateOf(instance) }
    var account by remember { mutableStateOf(me) }
    val allowSignup = info?.allowSignup
    val passwordReset = info?.passwordReset == true
    val baseUrl = hosted.session.baseUrl
    val client = remember { HostedClient() }
    if (instance == null && !hosted.session.signedIn) {
        LaunchedEffect(baseUrl) {
            info = null
            val url = hosted.session.normalizedUrl() ?: return@LaunchedEffect
            delay(400)
            info = try {
                withContext(Dispatchers.IO) { client.instance(url) }
            } catch (err: CancellationException) {
                throw err
            } catch (_: Exception) {
                null
            }
        }
    }
    if (me == null && hosted.session.signedIn) {
        LaunchedEffect(hosted.session.token) {
            account = try {
                withContext(Dispatchers.IO) { client.me(hosted.session) }
            } catch (err: CancellationException) {
                throw err
            } catch (_: Exception) {
                null
            }
        }
    }
    LaunchedEffect(allowSignup, passwordReset) {
        if (allowSignup == false && mode == HostedAuthMode.Create) mode = HostedAuthMode.SignIn
        if (info != null && !passwordReset && mode == HostedAuthMode.Reset) mode = HostedAuthMode.SignIn
    }
    var username by remember {
        mutableStateOf(hosted.session.username ?: hosted.session.expired?.username.orEmpty())
    }
    var password by remember { mutableStateOf("") }
    var email by remember { mutableStateOf("") }
    var login by remember { mutableStateOf("") }
    var sent by remember { mutableStateOf(false) }
    var editingEmail by remember { mutableStateOf(false) }
    var emailNotice by remember { mutableStateOf<String?>(null) }
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
            account?.takeIf { it.passwordReset }?.let { known ->
                EmailRow(trail = emailTrail(known), onClick = { editingEmail = true })
            }
            emailNotice?.let { message ->
                Text(
                    text = message,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
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
        } else if (mode == HostedAuthMode.Reset) {
            ResetForm(
                baseUrl = hosted.session.baseUrl,
                onBaseUrl = {
                    hosted.setBaseUrl(it)
                    error = null
                    sent = false
                },
                login = login,
                onLogin = {
                    login = it
                    error = null
                    sent = false
                },
                busy = busy,
                error = error,
                sent = sent,
                onSend = {
                    val url = hosted.session.normalizedUrl()
                    runAuth {
                        withContext(Dispatchers.IO) { client.requestReset(url.orEmpty(), login) }
                        sent = true
                    }
                },
                onSignIn = {
                    mode = HostedAuthMode.SignIn
                    error = null
                    sent = false
                },
            )
        } else {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 52.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                Text(
                    text = if (mode == HostedAuthMode.SignIn) "Sign in" else "Create account",
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
            if (mode == HostedAuthMode.Create && passwordReset) {
                OutlinedTextField(
                    value = email,
                    onValueChange = {
                        email = it
                        error = null
                    },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !busy,
                    singleLine = true,
                    label = { Text("Email (optional)") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                    textStyle = MaterialTheme.typography.bodyMedium,
                )
            }
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
                } else if (mode == HostedAuthMode.SignIn) {
                    "Sign in"
                } else {
                    "Create account"
                },
                enabled = ready,
                onClick = {
                    runAuth {
                        if (mode == HostedAuthMode.SignIn) {
                            hosted.signIn(username, password)
                        } else {
                            hosted.createAccount(username, password, email.takeIf { passwordReset })
                        }
                    }
                },
            )
            if (mode == HostedAuthMode.SignIn && passwordReset) {
                TextLink(
                    text = "Forgot password?",
                    enabled = !busy,
                    onClick = {
                        login = username.trim()
                        mode = HostedAuthMode.Reset
                        error = null
                        sent = false
                    },
                )
            }
            if (mode == HostedAuthMode.Create || showCreateAccount(allowSignup)) {
                TextLink(
                    text = if (mode == HostedAuthMode.SignIn) "Create an account" else "Sign in instead",
                    enabled = !busy,
                    onClick = {
                        mode = if (mode == HostedAuthMode.SignIn) HostedAuthMode.Create else HostedAuthMode.SignIn
                        error = null
                    },
                )
            }
        }
    }
    val known = account
    if (editingEmail && known != null) {
        EmailDialog(
            me = known,
            onSave = { next, current ->
                val wasSent = withContext(Dispatchers.IO) {
                    client.setEmail(hosted.session, next, current)
                }
                emailNotice = if (wasSent && next != null) emailSentNotice(next) else null
                editingEmail = false
                account = withContext(Dispatchers.IO) { client.me(hosted.session) }
            },
            onDismiss = { editingEmail = false },
        )
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
private fun ResetForm(
    baseUrl: String,
    onBaseUrl: (String) -> Unit,
    login: String,
    onLogin: (String) -> Unit,
    busy: Boolean,
    error: String?,
    sent: Boolean,
    onSend: () -> Unit,
    onSignIn: () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(text = "Reset password", style = MaterialTheme.typography.titleLarge)
    }
    OutlinedTextField(
        value = baseUrl,
        onValueChange = onBaseUrl,
        modifier = Modifier.fillMaxWidth(),
        enabled = !busy,
        singleLine = true,
        label = { Text("Server URL") },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
        textStyle = MaterialTheme.typography.bodyMedium,
    )
    OutlinedTextField(
        value = login,
        onValueChange = onLogin,
        modifier = Modifier.fillMaxWidth(),
        enabled = !busy,
        singleLine = true,
        label = { Text("Username or email") },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
        textStyle = MaterialTheme.typography.bodyMedium,
    )
    if (error != null) {
        Text(
            text = error,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.error,
            modifier = Modifier.fillMaxWidth(),
        )
    } else if (sent) {
        Text(
            text = RESET_SENT,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onBackground,
            modifier = Modifier.fillMaxWidth(),
        )
    }
    FormButton(
        modifier = Modifier.padding(top = 12.dp),
        text = if (busy) "Sending…" else "Send link",
        enabled = !busy && login.isNotBlank() && normalizeOrNull(baseUrl) != null,
        onClick = onSend,
    )
    TextLink(text = "Sign in instead", enabled = !busy, onClick = onSignIn)
}

internal const val RESET_SENT = "If that account has an email, we sent a link."

private fun normalizeOrNull(raw: String): String? =
    com.mewo.reader.data.normalizeBaseUrl(raw).ifBlank { null }

@Composable
private fun TextLink(
    text: String,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 40.dp)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = text,
            color = MaterialTheme.colorScheme.primary,
            style = MaterialTheme.typography.bodyLarge,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
private fun EmailRow(
    trail: String,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .clickable(role = Role.Button, onClick = onClick),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = "Email",
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.padding(end = 16.dp),
        )
        Text(
            text = trail,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.End,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
    }
}

internal fun emailSentNotice(email: String): String = "Check ${email.trim()} for a link."

/** Add, change or remove the account email. [onSave] throws the server's message. */
@Composable
internal fun EmailDialog(
    me: Me,
    onSave: suspend (email: String?, currentPassword: String) -> Unit,
    onDismiss: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var email by remember { mutableStateOf(me.pendingEmail ?: me.email.orEmpty()) }
    var current by remember { mutableStateOf("") }
    var saving by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val saved = me.email != null || me.pendingEmail != null

    fun change(next: String?) {
        if (current.isEmpty()) {
            error = "Enter your current password."
            return
        }
        scope.launch {
            saving = true
            error = null
            try {
                onSave(next, current)
            } catch (err: CancellationException) {
                throw err
            } catch (err: Exception) {
                error = err.message ?: "Could not reach the server."
            }
            saving = false
        }
    }

    AlertDialog(
        onDismissRequest = { if (!saving) onDismiss() },
        containerColor = MaterialTheme.colorScheme.surface,
        titleContentColor = MaterialTheme.colorScheme.onBackground,
        textContentColor = MaterialTheme.colorScheme.onSurfaceVariant,
        title = { Text(text = "Email", style = MaterialTheme.typography.titleLarge) },
        text = {
            Column {
                OutlinedTextField(
                    value = email,
                    onValueChange = {
                        email = it
                        error = null
                    },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !saving,
                    singleLine = true,
                    label = { Text("Email") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                    textStyle = MaterialTheme.typography.bodyMedium,
                )
                OutlinedTextField(
                    value = current,
                    onValueChange = {
                        current = it
                        error = null
                    },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !saving,
                    singleLine = true,
                    label = { Text("Current password") },
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    textStyle = MaterialTheme.typography.bodyMedium,
                )
                error?.let { message ->
                    Text(
                        text = message,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                }
            }
        },
        confirmButton = {
            TextButton(enabled = !saving && email.isNotBlank(), onClick = { change(email) }) {
                Text(if (saving) "Saving…" else "Save", color = MaterialTheme.colorScheme.primary)
            }
        },
        dismissButton = {
            if (saved) {
                TextButton(enabled = !saving, onClick = { change(null) }) {
                    Text("Remove email", color = MaterialTheme.colorScheme.error)
                }
            } else {
                TextButton(enabled = !saving, onClick = onDismiss) {
                    Text("Cancel", color = MaterialTheme.colorScheme.primary)
                }
            }
        },
    )
}

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
