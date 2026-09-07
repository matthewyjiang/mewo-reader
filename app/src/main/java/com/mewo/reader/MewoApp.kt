package com.mewo.reader

import android.app.Application
import com.mewo.reader.data.EpubOpener
import com.mewo.reader.data.LibraryRepository
import com.mewo.reader.data.ThemeStore

class MewoApp : Application() {
    lateinit var repository: LibraryRepository
        private set
    lateinit var themeStore: ThemeStore
        private set

    override fun onCreate() {
        super.onCreate()
        repository = LibraryRepository(
            context = this,
            opener = EpubOpener(this),
        )
        themeStore = ThemeStore(this)
    }
}
