package com.mewo.reader.ui.theme

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp

/** Theme swatches and Mewo mode. Lives on the Display settings page. */
@Composable
fun DisplaySettings(modifier: Modifier = Modifier) {
    val controller = LocalThemeController.current
    val mewoMode = LocalMewoMode.current
    var iconError by remember { mutableStateOf<String?>(null) }
    Column(modifier = modifier.fillMaxWidth()) {
        Text(
            text = "Twitter kept a navy night. X went black.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(20.dp))
        Column(
            modifier = Modifier.selectableGroup(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                ThemeSwatch(
                    theme = DisplayTheme.TwitterLight,
                    selected = controller.current == DisplayTheme.TwitterLight,
                    onSelect = { controller.setTheme(DisplayTheme.TwitterLight) },
                    modifier = Modifier.weight(1f),
                )
                ThemeSwatch(
                    theme = DisplayTheme.XLight,
                    selected = controller.current == DisplayTheme.XLight,
                    onSelect = { controller.setTheme(DisplayTheme.XLight) },
                    modifier = Modifier.weight(1f),
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                ThemeSwatch(
                    theme = DisplayTheme.TwitterDark,
                    selected = controller.current == DisplayTheme.TwitterDark,
                    onSelect = { controller.setTheme(DisplayTheme.TwitterDark) },
                    modifier = Modifier.weight(1f),
                )
                ThemeSwatch(
                    theme = DisplayTheme.XDark,
                    selected = controller.current == DisplayTheme.XDark,
                    onSelect = { controller.setTheme(DisplayTheme.XDark) },
                    modifier = Modifier.weight(1f),
                )
            }
        }
        Spacer(Modifier.height(24.dp))
        MewoModeRow(
            enabled = mewoMode.enabled,
            error = iconError,
            onToggle = { next ->
                try {
                    mewoMode.setEnabled(next)
                    iconError = null
                } catch (e: Exception) {
                    iconError = e.message?.takeIf { it.isNotBlank() }
                        ?: "Couldn't switch the app icon."
                }
            },
        )
    }
}

@Composable
private fun MewoModeRow(
    enabled: Boolean,
    error: String?,
    onToggle: (Boolean) -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .toggleable(
                    value = enabled,
                    onValueChange = onToggle,
                    role = Role.Switch,
                )
                .padding(vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text("Mewo mode", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(4.dp))
                Text(
                    text = "Use Teddy for the app icon, launch screen, and header. Your color theme stays the same.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Switch(
                checked = enabled,
                onCheckedChange = null,
            )
        }
        error?.let { message ->
            Spacer(Modifier.height(8.dp))
            Text(
                text = message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

@Composable
private fun ThemeSwatch(
    theme: DisplayTheme,
    selected: Boolean,
    onSelect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = theme.palette
    val shape = RoundedCornerShape(12.dp)
    Column(
        modifier = modifier
            .clip(shape)
            .border(
                width = 2.dp,
                color = if (selected) palette.accent else palette.line,
                shape = shape,
            )
            .background(palette.ground)
            .selectable(
                selected = selected,
                onClick = onSelect,
                role = Role.RadioButton,
            )
            .padding(12.dp),
    ) {
        Box(
            modifier = Modifier
                .size(24.dp)
                .clip(CircleShape)
                .background(if (selected) palette.accent else palette.line),
            contentAlignment = Alignment.Center,
        ) {
            if (selected) {
                Icon(
                    Icons.Outlined.Check,
                    contentDescription = null,
                    tint = palette.onAccent,
                    modifier = Modifier.size(14.dp),
                )
            }
        }
        Spacer(Modifier.height(20.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Box(
                modifier = Modifier
                    .size(10.dp)
                    .clip(CircleShape)
                    .background(palette.accent),
            )
            Box(
                modifier = Modifier
                    .size(10.dp)
                    .clip(CircleShape)
                    .background(palette.like),
            )
        }
        Spacer(Modifier.height(10.dp))
        Text(
            text = theme.label,
            style = MaterialTheme.typography.labelLarge,
            color = palette.ink,
        )
    }
}
