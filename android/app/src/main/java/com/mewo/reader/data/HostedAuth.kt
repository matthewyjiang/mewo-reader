package com.mewo.reader.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class HostedAuth(
    private val sessions: HostedSessionStore,
    private val client: HostedClient,
) {
    suspend fun signIn(username: String, password: String) {
        val url = requireUrl()
        val res = withContext(Dispatchers.IO) {
            client.login(url, username.trim(), password)
        }
        sessions.setSignedIn(res.username, res.token)
    }

    suspend fun createAccount(username: String, password: String) {
        val url = requireUrl()
        val res = withContext(Dispatchers.IO) {
            client.register(url, username.trim(), password)
        }
        sessions.setSignedIn(res.username, res.token)
    }

    suspend fun signOut() {
        val session = sessions.session.value
        sessions.signOut()
        if (session.signedIn) {
            withContext(Dispatchers.IO) {
                runCatching { client.logout(session) }
            }
        }
    }

    private fun requireUrl(): String {
        val url = sessions.session.value.normalizedUrl()
        if (url.isNullOrBlank()) {
            error("Set a server URL first.")
        }
        return url
    }
}
