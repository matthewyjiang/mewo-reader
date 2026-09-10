package com.mewo.reader.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.BackendKind
import com.mewo.reader.ui.LocalBackend

/** Local vs hosted library. Lives in the account drawer. */
@Composable
fun LibrarySettings(modifier: Modifier = Modifier) {
    val backend = LocalBackend.current
    Column(modifier = modifier.fillMaxWidth()) {
        Text("Library", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(6.dp))
        Text(
            text = "Books, likes, and where you left off. Local and hosted are different shelves.",
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
                body = "No server in this build, so this shelf stays empty.",
                selected = backend.kind == BackendKind.Hosted,
                onSelect = { backend.setKind(BackendKind.Hosted) },
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
