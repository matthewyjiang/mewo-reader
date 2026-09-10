package com.mewo.reader.ui

import androidx.compose.runtime.staticCompositionLocalOf
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.LibraryStore

data class BackendController(
    val kind: BackendKind,
    val setKind: (BackendKind) -> Unit,
)

val LocalBackend = staticCompositionLocalOf<BackendController> {
    error("BackendController missing")
}

val LocalLibraryStore = staticCompositionLocalOf<LibraryStore> {
    error("LibraryStore missing")
}
