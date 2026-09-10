package com.mewo.reader.data

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

data class HostedSession(
    val baseUrl: String = "",
    val username: String? = null,
    val token: String? = null,
) {
    val signedIn: Boolean
        get() = !token.isNullOrBlank() && normalizedUrl() != null

    fun normalizedUrl(): String? {
        val url = normalizeBaseUrl(baseUrl)
        return url.ifBlank { null }
    }
}

fun normalizeBaseUrl(raw: String): String {
    var url = raw.trim().trimEnd('/')
    if (url.isEmpty()) return ""
    if (!url.startsWith("http://") && !url.startsWith("https://")) {
        url = "http://$url"
    }
    return url
}

class HostedSessionStore(context: Context) {
    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val _session = MutableStateFlow(read())
    val session: StateFlow<HostedSession> = _session.asStateFlow()

    fun setBaseUrl(url: String) {
        prefs.edit().putString(KEY_URL, url).apply()
        _session.update { it.copy(baseUrl = url) }
    }

    fun setSignedIn(username: String, token: String) {
        prefs.edit()
            .putString(KEY_USER, username)
            .putString(KEY_TOKEN, token)
            .apply()
        _session.update { it.copy(username = username, token = token) }
    }

    fun signOut() {
        prefs.edit()
            .remove(KEY_USER)
            .remove(KEY_TOKEN)
            .apply()
        _session.update { it.copy(username = null, token = null) }
    }

    private fun read(): HostedSession = HostedSession(
        baseUrl = prefs.getString(KEY_URL, "").orEmpty(),
        username = prefs.getString(KEY_USER, null),
        token = prefs.getString(KEY_TOKEN, null),
    )

    companion object {
        private const val PREFS = "mewo_hosted"
        private const val KEY_URL = "base_url"
        private const val KEY_USER = "username"
        private const val KEY_TOKEN = "token"
    }
}
