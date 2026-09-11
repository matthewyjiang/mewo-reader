package com.mewo.reader.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.BackendKind
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalHostedAuth
import kotlinx.coroutines.launch

/** Local vs hosted library. Lives on the Library settings page. */
@Composable
fun LibrarySettings(modifier: Modifier = Modifier) {
    val backend = LocalBackend.current
    val hosted = LocalHostedAuth.current
    Column(modifier = modifier.fillMaxWidth()) {
        Text(
            text = "Books, likes, notes, and where you left off. Local is yours. Hosted is a shared shelf.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        Column(
            modifier = Modifier.selectableGroup(),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            BackendRow(
                title = "On this phone",
                body = "EPUB files stay on the device.",
                selected = backend.kind == BackendKind.Local,
                onSelect = { backend.setKind(BackendKind.Local) },
            )
            BackendRow(
                title = "Hosted server",
                body = if (hosted.session.signedIn) {
                    "Signed in as @${hosted.session.username}."
                } else {
                    "A shared shelf. Sign in with a server URL."
                },
                selected = backend.kind == BackendKind.Hosted,
                onSelect = { backend.setKind(BackendKind.Hosted) },
            )
        }
        if (backend.kind == BackendKind.Hosted) {
            Spacer(Modifier.height(16.dp))
            HostedAccountForm()
        }
    }
}

@Composable
private fun HostedAccountForm() {
    val hosted = LocalHostedAuth.current
    val scope = rememberCoroutineScope()
    var username by remember { mutableStateOf(hosted.session.username.orEmpty()) }
    var password by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

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
        OutlinedTextField(
            value = hosted.session.baseUrl,
            onValueChange = hosted.setBaseUrl,
            modifier = Modifier.fillMaxWidth(),
            enabled = !hosted.session.signedIn && !busy,
            singleLine = true,
            label = { Text("Server URL") },
            placeholder = { Text("http://192.168.1.10:8787") },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            textStyle = MaterialTheme.typography.bodyMedium,
        )
        if (hosted.session.signedIn) {
            Spacer(Modifier.height(12.dp))
            TextButton(
                onClick = { runAuth { hosted.signOut() } },
                enabled = !busy,
            ) {
                Text("Sign out")
            }
        } else {
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = username,
                onValueChange = { username = it },
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy,
                singleLine = true,
                label = { Text("Username") },
                textStyle = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = password,
                onValueChange = { password = it },
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy,
                singleLine = true,
                label = { Text("Password") },
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                textStyle = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(4.dp))
            Row {
                TextButton(
                    onClick = { runAuth { hosted.signIn(username, password) } },
                    enabled = !busy,
                ) {
                    Text("Sign in")
                }
                TextButton(
                    onClick = { runAuth { hosted.createAccount(username, password) } },
                    enabled = !busy,
                ) {
                    Text("Create account")
                }
            }
        }
        error?.let { message ->
            Spacer(Modifier.height(4.dp))
            Text(
                text = message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

@Composable
private fun BackendRow(
    title: String,
    body: String,
    selected: Boolean,
    onSelect: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .selectable(
                selected = selected,
                onClick = onSelect,
                role = Role.RadioButton,
            )
            .padding(vertical = 8.dp),
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
        Spacer(Modifier.height(4.dp))
        Text(
            text = body,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
