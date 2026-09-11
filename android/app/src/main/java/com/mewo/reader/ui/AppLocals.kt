package com.mewo.reader.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.HostedSession
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.LocalProfile
import com.mewo.reader.data.PostComment

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

data class LocalProfileController(
    val profile: LocalProfile,
    val save: (name: String, handle: String) -> Unit,
)

data class ReaderIdentity(
    val name: String,
    val handle: String,
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

val LocalProfile = staticCompositionLocalOf<LocalProfileController> {
    error("LocalProfileController missing")
}

/** Hosted username when signed in on that shelf. Local name and handle otherwise. */
@Composable
fun currentReaderIdentity(): ReaderIdentity {
    val profile = LocalProfile.current.profile
    val hosted = LocalBackend.current.kind == BackendKind.Hosted
    if (hosted) {
        val username = LocalHostedAuth.current.session.username
        if (!username.isNullOrBlank()) {
            return ReaderIdentity(name = username, handle = username)
        }
    }
    return ReaderIdentity(name = profile.displayName, handle = profile.handle)
}

/** Local notes use the saved profile. Hosted notes use the comment username. */
@Composable
fun commenterIdentity(comment: PostComment): ReaderIdentity {
    val local = LocalBackend.current.kind != BackendKind.Hosted
    if (local) return currentReaderIdentity()
    return ReaderIdentity(name = comment.username, handle = comment.username)
}
