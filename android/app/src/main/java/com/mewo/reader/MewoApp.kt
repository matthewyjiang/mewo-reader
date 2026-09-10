package com.mewo.reader

import android.app.Application
import com.mewo.reader.data.BackendStore
import com.mewo.reader.data.EpubOpener
import com.mewo.reader.data.HostedLibraryStore
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.LocalLibraryStore
import com.mewo.reader.data.MewoModeStore
import com.mewo.reader.data.SwitchingLibraryStore
import com.mewo.reader.data.ThemeStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

class MewoApp : Application() {
    private val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    lateinit var library: LibraryStore
        private set
    lateinit var backendStore: BackendStore
        private set
    lateinit var themeStore: ThemeStore
        private set
    lateinit var mewoModeStore: MewoModeStore
        private set

    override fun onCreate() {
        super.onCreate()
        backendStore = BackendStore(this)
        library = SwitchingLibraryStore(
            local = LocalLibraryStore(
                context = this,
                opener = EpubOpener(this),
            ),
            hosted = HostedLibraryStore(),
            kind = backendStore.kind,
            scope = appScope,
        )
        themeStore = ThemeStore(this)
        mewoModeStore = MewoModeStore(this)
    }
}
