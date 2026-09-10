package com.mewo.reader.data

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

enum class BackendKind(val id: String) {
    Local("local"),
    Hosted("hosted"),
    ;

    companion object {
        fun fromId(id: String?): BackendKind = entries.find { it.id == id } ?: Local
    }
}

class BackendStore(context: Context) {
    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val _kind = MutableStateFlow(read())
    val kind: StateFlow<BackendKind> = _kind.asStateFlow()

    fun setKind(kind: BackendKind) {
        prefs.edit().putString(KEY, kind.id).apply()
        _kind.value = kind
    }

    private fun read(): BackendKind = BackendKind.fromId(prefs.getString(KEY, null))

    companion object {
        private const val PREFS = "mewo"
        private const val KEY = "backend_kind"
    }
}
