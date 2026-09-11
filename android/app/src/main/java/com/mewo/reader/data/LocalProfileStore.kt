package com.mewo.reader.data

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class LocalProfileStore(context: Context) {
    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val _profile = MutableStateFlow(read())
    val profile: StateFlow<LocalProfile> = _profile.asStateFlow()

    fun save(name: String, handle: String) {
        val parsed = LocalIdentity.parse(name, handle).getOrThrow()
        prefs.edit()
            .putString(KEY_NAME, parsed.name)
            .putString(KEY_HANDLE, parsed.handle)
            .apply()
        _profile.value = parsed
    }

    private fun read(): LocalProfile {
        val handle = prefs.getString(KEY_HANDLE, "").orEmpty()
        if (handle.isBlank()) return LocalProfile()
        val name = prefs.getString(KEY_NAME, "").orEmpty()
        return LocalIdentity.parse(name, handle).getOrDefault(LocalProfile())
    }

    companion object {
        private const val PREFS = "mewo_profile"
        private const val KEY_NAME = "name"
        private const val KEY_HANDLE = "handle"
    }
}
