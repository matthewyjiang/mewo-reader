package com.mewo.reader.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.HostedSession
import com.mewo.reader.data.LocalProfile
import com.mewo.reader.ui.components.BrandMark
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileButton
import com.mewo.reader.ui.library.BookPost
import com.mewo.reader.ui.library.SectionLabel
import com.mewo.reader.ui.onboarding.OnboardingScreen
import com.mewo.reader.ui.settings.SettingsHome
import com.mewo.reader.ui.theme.DisplayTheme
import com.mewo.reader.ui.theme.LocalMewoColors
import com.mewo.reader.ui.theme.LocalMewoMode
import com.mewo.reader.ui.theme.LocalThemeController
import com.mewo.reader.ui.theme.MewoModeController
import com.mewo.reader.ui.theme.ThemeController
import com.mewo.reader.ui.theme.mewoTypography
import com.mewo.reader.ui.theme.toColorScheme
import org.junit.Rule
import org.junit.Test

/**
 * Layout goldens for the surfaces an agent can check without an emulator.
 * Phone is a Pixel 5. Narrow is the same height at 320dp wide.
 */
class SurfaceScreenshots {
    @get:Rule
    val paparazzi = Paparazzi(
        deviceConfig = phone,
        theme = "android:Theme.Material.NoActionBar",
    )

    @Test
    fun hostedLibrary() = shoot("library") { HostedLibrary() }

    @Test
    fun settings() = shoot("settings") {
        SettingsHome(
            onBack = {},
            onOpenAccount = {},
            onOpenDisplay = {},
            onOpenLibrary = {},
        )
    }

    @Test
    fun onboarding() = shoot("onboarding") { OnboardingScreen() }

    private fun shoot(name: String, content: @Composable () -> Unit) {
        val profile = if (name == "onboarding") {
            LocalProfile()
        } else {
            LocalProfile(name = "Ada Lovelace", handle = "ada")
        }
        for ((deviceName, device) in devices) {
            paparazzi.unsafeUpdateConfig(deviceConfig = device)
            for ((themeName, theme) in themes) {
                paparazzi.snapshot(name = "${name}_${deviceName}_$themeName") {
                    Frame(theme = theme, profile = profile, content = content)
                }
            }
        }
    }
}

private val phone = DeviceConfig.PIXEL_5.copy(softButtons = false)

/** 320dp wide at the Pixel 5 density (440dpi). */
private val narrow = phone.copy(screenWidth = 880)

private val devices = listOf(
    "phone" to phone,
    "narrow" to narrow,
)

private val themes = listOf(
    "light" to DisplayTheme.XLight,
    "dark" to DisplayTheme.XDark,
)

@Composable
private fun Frame(
    theme: DisplayTheme,
    profile: LocalProfile,
    content: @Composable () -> Unit,
) {
    val palette = theme.palette
    CompositionLocalProvider(
        LocalMewoColors provides palette,
        LocalThemeController provides ThemeController(current = theme, setTheme = {}),
        LocalMewoMode provides MewoModeController(enabled = false, setEnabled = {}),
        LocalProfile provides LocalProfileController(profile = profile, save = { _, _ -> }),
        LocalBackend provides BackendController(kind = BackendKind.Hosted, setKind = {}),
        LocalHostedAuth provides HostedAuthController(
            session = HostedSession(
                baseUrl = "https://mewo.example",
                username = "ada",
                token = "token",
            ),
            setBaseUrl = {},
            signIn = { _, _ -> },
            createAccount = { _, _ -> },
            signOut = {},
        ),
    ) {
        MaterialTheme(
            colorScheme = palette.toColorScheme(),
            typography = mewoTypography(palette.ink),
            content = content,
        )
    }
}

@Composable
private fun HostedLibrary() {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        MewoAppBar(
            leading = { ProfileButton(onClick = {}) },
            center = { BrandMark() },
        )
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState()),
        ) {
            SectionLabel("My books")
            shelfBook(privateBook)
            shelfBook(sharedBook)
            shelfBook(longTitleBook)
            SectionLabel("Shared with me")
            shelfBook(sharedWithMe)
        }
    }
}

@Composable
private fun shelfBook(book: BookRecord) {
    val mine = book.mine
    BookPost(
        book = book,
        cover = null,
        showOwner = !mine,
        onOpen = {},
        onDelete = if (mine) ({ }) else null,
        onShare = if (mine) ({ _ -> }) else null,
    )
    HorizontalDivider(
        color = MaterialTheme.colorScheme.outline,
        thickness = 0.6.dp,
    )
}

private val privateBook = book(
    id = "private",
    title = "Pride and Prejudice",
    shared = false,
)

private val sharedBook = book(
    id = "shared",
    title = "Emma",
    shared = true,
)

private val longTitleBook = book(
    id = "long",
    title = "The Particular Sadness of Lemon Cake and Other Stories from a Very Long Shelf",
    shared = false,
)

private val sharedWithMe = book(
    id = "theirs",
    title = "Persuasion",
    author = "Austen",
    handle = "austen",
    mine = false,
    shared = true,
    ownerUsername = "ada",
)

private fun book(
    id: String,
    title: String,
    author: String = "Jane Austen",
    handle: String = "austen",
    mine: Boolean = true,
    shared: Boolean = false,
    ownerUsername: String? = null,
) = BookRecord(
    id = id,
    title = title,
    author = author,
    handle = handle,
    importedAt = 1,
    postCount = 120,
    progressIndex = 40,
    mine = mine,
    shared = shared,
    ownerUsername = ownerUsername,
)
