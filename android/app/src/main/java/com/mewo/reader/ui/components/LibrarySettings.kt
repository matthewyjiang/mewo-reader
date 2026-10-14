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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.BackendKind
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalHostedAuth
import kotlinx.coroutines.launch

private val usernamePattern = Regex("^[A-Za-z0-9_]{3,32}$")
private const val USERNAME_RULE = "3–32 characters: letters, digits, underscore."
private const val PASSWORD_RULE = "At least 8 characters."

private enum class AuthMode { SignIn, Create }

/** Local vs hosted library. Lives on the Library settings page. */
@Composable
fun LibrarySettings(
    modifier: Modifier = Modifier,
    startOnCreate: Boolean = false,
) {
    val backend = LocalBackend.current
    val hosted = LocalHostedAuth.current
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
            HostedAccountForm(startOnCreate = startOnCreate)
        }
    }
}

@Composable
private fun HostedAccountForm(startOnCreate: Boolean) {
    val hosted = LocalHostedAuth.current
    val scope = rememberCoroutineScope()
    var mode by remember { mutableStateOf(if (startOnCreate) AuthMode.Create else AuthMode.SignIn) }
    var username by remember { mutableStateOf(hosted.session.username.orEmpty()) }
    var password by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
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
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 52.dp)
                    .clickable(enabled = !busy, role = Role.Button) {
                        runAuth { hosted.signOut() }
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
