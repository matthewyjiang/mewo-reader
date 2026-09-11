package com.mewo.reader

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mewo.reader.ui.BackendController
import com.mewo.reader.ui.HostedAuthController
import com.mewo.reader.ui.LocalBackend
import com.mewo.reader.ui.LocalHostedAuth
import com.mewo.reader.ui.LocalLibraryStore
import com.mewo.reader.ui.LocalProfile
import com.mewo.reader.ui.LocalProfileController
import com.mewo.reader.ui.MewoNav
import com.mewo.reader.ui.onboarding.OnboardingScreen
import com.mewo.reader.ui.theme.LocalMewoMode
import com.mewo.reader.ui.theme.LocalReaderFont
import com.mewo.reader.ui.theme.LocalThemeController
import com.mewo.reader.ui.theme.MewoModeController
import com.mewo.reader.ui.theme.MewoTheme
import com.mewo.reader.ui.theme.ReaderFontController
import com.mewo.reader.ui.theme.ThemeController

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            /*
             * THESIS: A book is a timeline. Refuse the typeset page.
             * OWN-WORLD: Four display palettes, Twitter or X, light or night.
             * Hairline rules, circular avatars, Atkinson body.
             * STORY: Import an EPUB, scroll the author as a feed, like a line.
             * FIRST VIEWPORT: Sticky "Mewo" bar, feed of books or empty timeline
             * plus one FAB to add a file. Avatar opens the account drawer.
             * FORM: X home timeline, brief-pinned, seed skipped.
             * FINISH: unreviewed and undocumented is unfinished; this build ends
             * with the finish review, the verdict, and DESIGN.md
             */
            val app = application as MewoApp
            val theme by app.themeStore.theme.collectAsStateWithLifecycle()
            val mewoMode by app.mewoModeStore.enabled.collectAsStateWithLifecycle()
            val readerFont by app.readerFontStore.scale.collectAsStateWithLifecycle()
            val backend by app.backendStore.kind.collectAsStateWithLifecycle()
            val hosted by app.hostedSession.session.collectAsStateWithLifecycle()
            val profile by app.localProfileStore.profile.collectAsStateWithLifecycle()
            MewoTheme(theme = theme) {
                CompositionLocalProvider(
                    LocalThemeController provides ThemeController(
                        current = theme,
                        setTheme = app.themeStore::setTheme,
                    ),
                    LocalMewoMode provides MewoModeController(
                        enabled = mewoMode,
                        setEnabled = app.mewoModeStore::setEnabled,
                    ),
                    LocalReaderFont provides ReaderFontController(
                        scale = readerFont,
                        setStep = app.readerFontStore::setStep,
                    ),
                    LocalBackend provides BackendController(
                        kind = backend,
                        setKind = app.backendStore::setKind,
                    ),
                    LocalHostedAuth provides HostedAuthController(
                        session = hosted,
                        setBaseUrl = app.hostedSession::setBaseUrl,
                        signIn = app.hostedAuth::signIn,
                        createAccount = app.hostedAuth::createAccount,
                        signOut = app.hostedAuth::signOut,
                    ),
                    LocalLibraryStore provides app.library,
                    LocalProfile provides LocalProfileController(
                        profile = profile,
                        save = app.localProfileStore::save,
                    ),
                ) {
                    if (profile.ready) {
                        MewoNav()
                    } else {
                        OnboardingScreen()
                    }
                }
            }
        }
    }
}
