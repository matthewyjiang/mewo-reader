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
import androidx.compose.runtime.LaunchedEffect
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.mewo.reader.BuildConfig
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.HostedClient
import com.mewo.reader.data.LocalIdentity
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalHostedAuth
import com.mewo.reader.ui.LocalProfile
import com.mewo.reader.ui.components.LibrarySettings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileForm
import com.mewo.reader.ui.components.ProfilePreview
import com.mewo.reader.ui.theme.DisplaySettings

object SettingsRoutes {
    const val Home = "settings"
    const val Account = "settings/account"
    const val Display = "settings/display"
    const val Library = "settings/library"
    const val About = "settings/about"

    fun matches(route: String) = route.startsWith("settings")
}

data class AppBuild(
    val versionName: String,
    val versionCode: Int,
    val commit: String?,
) {
    companion object {
        fun current(): AppBuild = AppBuild(
            versionName = BuildConfig.VERSION_NAME,
            versionCode = BuildConfig.VERSION_CODE,
            commit = BuildConfig.COMMIT.takeIf { it.isNotBlank() },
        )
    }
}

sealed interface ServerVersionState {
    data object Loading : ServerVersionState
    data object Unavailable : ServerVersionState
    data class Ready(val version: String, val commit: String?) : ServerVersionState
}

internal fun formatAppVersion(name: String, code: Int, commit: String?): String {
    val sha = commit?.takeIf { it.isNotBlank() }
    return if (sha == null) "$name ($code)" else "$name ($code, $sha)"
}

internal fun formatServerVersion(version: String, commit: String?): String {
    val sha = commit?.takeIf { it.isNotBlank() }
    return if (sha == null) version else "$version ($sha)"
}

@Composable
fun SettingsHome(
    onBack: () -> Unit,
    onOpenAccount: () -> Unit,
    onOpenDisplay: () -> Unit,
    onOpenLibrary: () -> Unit,
    onOpenAbout: () -> Unit,
) {
    SettingsScaffold(title = "Settings", onBack = onBack) {
        SettingsCategoryRow(title = "Account", onClick = onOpenAccount)
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
        SettingsCategoryRow(title = "Display", onClick = onOpenDisplay)
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
        SettingsCategoryRow(title = "Library", onClick = onOpenLibrary)
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
        SettingsCategoryRow(title = "About", onClick = onOpenAbout)
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
    }
}

@Composable
fun SettingsAboutPage(
    onBack: () -> Unit,
    app: AppBuild = AppBuild.current(),
    server: ServerVersionState? = null,
) {
    val resolved = server ?: rememberServerVersion()
    val url = LocalHostedAuth.current.session.normalizedUrl()
    SettingsScaffold(title = "About", onBack = onBack) {
        AboutFact(
            label = "App",
            value = formatAppVersion(app.versionName, app.versionCode, app.commit),
        )
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
        AboutFact(
            label = "Server",
            value = when (resolved) {
                ServerVersionState.Loading -> "Loading…"
                ServerVersionState.Unavailable -> "Unavailable"
                is ServerVersionState.Ready -> formatServerVersion(resolved.version, resolved.commit)
            },
            muted = resolved !is ServerVersionState.Ready,
        )
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
        AboutFact(
            label = "Server URL",
            value = url ?: "Unavailable",
            muted = url == null,
        )
        HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
    }
}

@Composable
private fun rememberServerVersion(): ServerVersionState {
    val hosted = LocalBackend.current.kind == BackendKind.Hosted
    val url = LocalHostedAuth.current.session.normalizedUrl()
    var state by remember(hosted, url) {
        mutableStateOf(
            if (hosted && url != null) ServerVersionState.Loading else ServerVersionState.Unavailable,
        )
    }
    if (hosted && url != null) {
        val client = remember { HostedClient() }
        LaunchedEffect(url) {
            state = withContext(Dispatchers.IO) {
                runCatching { client.version(url) }
                    .mapCatching { info ->
                        if (info.version.isBlank()) error("empty version")
                        ServerVersionState.Ready(
                            version = info.version,
                            commit = info.commit?.takeIf { it.isNotBlank() },
                        )
                    }
                    .getOrElse { ServerVersionState.Unavailable }
            }
        }
    }
    return state
}

@Composable
private fun AboutFact(
    label: String,
    value: String,
    muted: Boolean = false,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.weight(1f, fill = false),
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyLarge,
            color = if (muted) {
                MaterialTheme.colorScheme.onSurfaceVariant
            } else {
                MaterialTheme.colorScheme.onSurface
            },
            textAlign = TextAlign.End,
            modifier = Modifier.weight(1f),
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
            modifier = Modifier.padding(horizontal = 16.dp),
        ) {
            if (name.isNotBlank() || handle.isNotBlank()) {
                ProfilePreview(name = name, handle = handle)
                Spacer(Modifier.size(12.dp))
            }
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
            SavePill(
                enabled = canSave,
                modifier = Modifier.fillMaxWidth(),
                onClick = {
                    parsed
                        .onSuccess { store.save(it.name, it.handle) }
                        .onFailure { err ->
                            error = err.message ?: "Could not save that."
                        }
                },
            )
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
        DisplaySettings()
    }
}

@Composable
fun SettingsLibraryPage(
    onBack: () -> Unit,
    startOnCreate: Boolean = false,
) {
    SettingsScaffold(title = "Library", onBack = onBack) {
        LibrarySettings(
            modifier = Modifier.padding(horizontal = 16.dp),
            startOnCreate = startOnCreate,
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
    modifier: Modifier = Modifier,
) {
    val accent = MaterialTheme.colorScheme.primary
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(999.dp))
            .background(if (enabled) accent else accent.copy(alpha = 0.35f))
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 10.dp)
            .semantics { contentDescription = "Save" },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = "Save",
            color = MaterialTheme.colorScheme.onPrimary,
            style = MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.Bold,
        )
    }
}

@Composable
private fun SettingsCategoryRow(
    title: String,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.weight(1f),
        )
        Icon(
            imageVector = Icons.AutoMirrored.Outlined.KeyboardArrowRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(22.dp),
        )
    }
}
