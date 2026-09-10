package com.mewo.reader.ui

import androidx.compose.runtime.staticCompositionLocalOf
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.HostedSession
import com.mewo.reader.data.LibraryStore

data class BackendController(
    val kind: BackendKind,
    val setKind: (BackendKind) -> Unit,
)

data class HostedAuthController(
    val session: HostedSession,
    val setBaseUrl: (String) -> Unit,
    val signIn: suspend (username: String, password: String) -> Unit,
    val createAccount: suspend (username: String, password: String) -> Unit,
    val signOut: suspend () -> Unit,
)

val LocalBackend = staticCompositionLocalOf<BackendController> {
    error("BackendController missing")
}

val LocalHostedAuth = staticCompositionLocalOf<HostedAuthController> {
    error("HostedAuthController missing")
}

val LocalLibraryStore = staticCompositionLocalOf<LibraryStore> {
    error("LibraryStore missing")
}
