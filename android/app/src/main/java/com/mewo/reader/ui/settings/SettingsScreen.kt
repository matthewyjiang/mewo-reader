package com.mewo.reader.ui.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.LocalIdentity
import com.mewo.reader.ui.LocalProfile
import com.mewo.reader.ui.components.LibrarySettings
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileForm
import com.mewo.reader.ui.components.ProfilePreview
import com.mewo.reader.ui.theme.DisplaySettings

object SettingsRoutes {
    const val Home = "settings"
    const val Account = "settings/account"
    const val Display = "settings/display"
    const val Library = "settings/library"

    fun matches(route: String) = route.startsWith("settings")
}

@Composable
fun SettingsHome(
    onBack: () -> Unit,
    onOpenAccount: () -> Unit,
    onOpenDisplay: () -> Unit,
    onOpenLibrary: () -> Unit,
) {
    val profile = LocalProfile.current.profile
    SettingsScaffold(title = "Settings", onBack = onBack) {
        SettingsCategoryRow(
            title = "Account",
            body = if (profile.ready) "@${profile.handle}" else "Name and handle.",
            onClick = onOpenAccount,
        )
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
        SettingsCategoryRow(
            title = "Display",
            body = "Twitter or X. Light or night. Font size. Mewo mode.",
            onClick = onOpenDisplay,
        )
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
        SettingsCategoryRow(
            title = "Library",
            body = "On this phone, or a hosted server.",
            onClick = onOpenLibrary,
        )
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
    }
}

@Composable
fun SettingsAccountPage(onBack: () -> Unit) {
    val store = LocalProfile.current
    var name by remember { mutableStateOf(store.profile.name) }
    var handle by remember { mutableStateOf(store.profile.handle) }
    var error by remember { mutableStateOf<String?>(null) }
    val parsed = LocalIdentity.parse(name, handle)
    val dirty = name.trim() != store.profile.name ||
        LocalIdentity.normalizeHandle(handle) != store.profile.handle
    val canSave = parsed.isSuccess && dirty

    SettingsScaffold(title = "Account", onBack = onBack) {
        Column(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 16.dp),
        ) {
            ProfilePreview(name = name, handle = handle)
            Spacer(Modifier.size(20.dp))
            ProfileForm(
                name = name,
                handle = handle,
                onNameChange = {
                    name = it
                    error = null
                },
                onHandleChange = {
                    handle = it
                    error = null
                },
            )
            Spacer(Modifier.size(16.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End,
            ) {
                SavePill(
                    enabled = canSave,
                    onClick = {
                        parsed
                            .onSuccess { store.save(it.name, it.handle) }
                            .onFailure { err ->
                                error = err.message ?: "Could not save that."
                            }
                    },
                )
            }
            error?.let { message ->
                Spacer(Modifier.size(8.dp))
                Text(
                    text = message,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.error,
                )
            }
        }
    }
}

@Composable
fun SettingsDisplayPage(onBack: () -> Unit) {
    SettingsScaffold(title = "Display", onBack = onBack) {
        DisplaySettings(
            modifier = Modifier.padding(vertical = 16.dp),
        )
    }
}

@Composable
fun SettingsLibraryPage(onBack: () -> Unit) {
    SettingsScaffold(title = "Library", onBack = onBack) {
        LibrarySettings(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 16.dp),
        )
    }
}

@Composable
private fun SettingsScaffold(
    title: String,
    onBack: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        SettingsBar(title = title, onBack = onBack)
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(bottom = 28.dp),
            content = content,
        )
    }
}

@Composable
private fun SettingsBar(
    title: String,
    onBack: () -> Unit,
) {
    MewoAppBar(
        leading = {
            IconButton(onClick = onBack) {
                Icon(
                    Icons.AutoMirrored.Outlined.ArrowBack,
                    contentDescription = "Back",
                    tint = MaterialTheme.colorScheme.onBackground,
                )
            }
        },
        center = {
            Box(
                modifier = Modifier
                    .fillMaxHeight()
                    .padding(horizontal = 56.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleSmall,
                )
            }
        },
    )
}

@Composable
private fun SavePill(
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val accent = MaterialTheme.colorScheme.primary
    Text(
        text = "Save",
        color = MaterialTheme.colorScheme.onPrimary,
        style = MaterialTheme.typography.labelLarge,
        fontWeight = FontWeight.Bold,
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(if (enabled) accent else accent.copy(alpha = 0.35f))
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp)
            .semantics { contentDescription = "Save" },
    )
}

@Composable
private fun SettingsCategoryRow(
    title: String,
    body: String,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.size(4.dp))
            Text(
                text = body,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Icon(
            imageVector = Icons.AutoMirrored.Outlined.KeyboardArrowRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(22.dp),
        )
    }
}
