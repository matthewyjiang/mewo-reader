package com.mewo.reader.data

import android.content.Context
import android.os.Looper
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

data class HostedSession(
    val baseUrl: String = "",
    val username: String? = null,
    val token: String? = null,
    /** Set by a 401. [HostedSessionStore.signOut] and [HostedSessionStore.setSignedIn] clear it. */
    val expired: OutboxAccount? = null,
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
    private val sessionLock = Any()
    private val _session = MutableStateFlow(read())
    val session: StateFlow<HostedSession> = _session.asStateFlow()

    fun setBaseUrl(url: String) {
        synchronized(sessionLock) {
            prefs.edit().putString(KEY_URL, url).apply()
            _session.update { it.copy(baseUrl = url) }
        }
    }

    fun setSignedIn(username: String, token: String) {
        synchronized(sessionLock) {
            prefs.edit()
                .putString(KEY_USER, username)
                .putString(KEY_TOKEN, token)
                .remove(KEY_EXPIRED_URL)
                .remove(KEY_EXPIRED_USER)
                .apply()
            _session.update { it.copy(username = username, token = token, expired = null) }
            val url = _session.value.normalizedUrl()
            val account = if (url != null && username.isNotBlank()) OutboxAccount(url, username) else null
            // Record the owner only once the session shows this account. Doing it earlier makes
            // an in-flight signed-out collection look like a wipe. A later account does not
            // replace the owner until cleanup finishes.
            if (diskOwner() == null && account != null) setDiskOwner(account)
        }
    }

    fun signOut() {
        synchronized(sessionLock) { signOutLocked() }
    }

    /**
     * A 401 for [token]. No-op unless the session is still signed in with that exact token,
     * so a second 401 cannot clear [HostedSession.expired] and wipe the downloads.
     * A signed-in session with no username still signs out.
     */
    fun expire(token: String) {
        synchronized(sessionLock) {
            val current = _session.value
            if (current.token != token || !current.signedIn) return
            val url = current.normalizedUrl()
            val user = current.username?.takeIf { it.isNotBlank() }
            if (url == null || user == null) {
                signOutLocked()
                return
            }
            val account = OutboxAccount(url, user)
            prefs.edit()
                .remove(KEY_USER)
                .remove(KEY_TOKEN)
                .putString(KEY_EXPIRED_URL, account.url)
                .putString(KEY_EXPIRED_USER, account.username)
                .apply()
            _session.value = current.copy(username = null, token = null, expired = account)
        }
    }

    private fun signOutLocked() {
        prefs.edit()
            .remove(KEY_USER)
            .remove(KEY_TOKEN)
            .remove(KEY_EXPIRED_URL)
            .remove(KEY_EXPIRED_USER)
            .apply()
        _session.update { it.copy(username = null, token = null, expired = null) }
    }

    /** Whose shelf and downloads are on disk. Sign-in does not clear this; cleanup does. */
    fun diskOwner(): OutboxAccount? {
        val url = prefs.getString(KEY_FILES_URL, null)?.takeIf { it.isNotBlank() } ?: return null
        val user = prefs.getString(KEY_FILES_USER, null)?.takeIf { it.isNotBlank() } ?: return null
        return OutboxAccount(normalizeBaseUrl(url), user)
    }

    fun setDiskOwner(account: OutboxAccount?) {
        if (diskOwner() == account) return
        val editor = prefs.edit()
        if (account == null) {
            editor.remove(KEY_FILES_URL).remove(KEY_FILES_USER)
        } else {
            editor.putString(KEY_FILES_URL, account.url).putString(KEY_FILES_USER, account.username)
        }
        // commit() on the main thread froze the UI. Off the main thread it still
        // finishes before the caller continues, so a kill keeps this owner.
        if (Looper.myLooper() == Looper.getMainLooper()) editor.apply() else editor.commit()
    }

    private fun read(): HostedSession {
        val expiredUrl = prefs.getString(KEY_EXPIRED_URL, null)?.takeIf { it.isNotBlank() }
        val expiredUser = prefs.getString(KEY_EXPIRED_USER, null)?.takeIf { it.isNotBlank() }
        val expired = if (expiredUrl != null && expiredUser != null) {
            OutboxAccount(normalizeBaseUrl(expiredUrl), expiredUser)
        } else {
            null
        }
        return HostedSession(
            baseUrl = prefs.getString(KEY_URL, "").orEmpty(),
            username = prefs.getString(KEY_USER, null),
            token = prefs.getString(KEY_TOKEN, null),
            expired = expired,
        )
    }

    companion object {
        private const val PREFS = "mewo_hosted"
        private const val KEY_URL = "base_url"
        private const val KEY_USER = "username"
        private const val KEY_TOKEN = "token"
        private const val KEY_EXPIRED_URL = "expired_url"
        private const val KEY_EXPIRED_USER = "expired_user"
        private const val KEY_FILES_URL = "files_url"
        private const val KEY_FILES_USER = "files_user"
    }
}

/** Who this session says the files belong to. Signed out, that is [HostedSession.expired]. */
internal fun HostedSession.filesOwner(): OutboxAccount? = outboxAccount() ?: expired

internal sealed interface SessionChange {
    data object Keep : SessionChange
    data object Expired : SessionChange
    data class OtherUser(val old: OutboxAccount) : SessionChange

    /** [old] is null when a signed-in session had no username. The download directory still goes. */
    data class Wipe(val old: OutboxAccount?) : SessionChange
}

/**
 * [owner] is the account whose files are still on disk, or null to use [previous].
 * A signed-in session with no username wipes on the way out. That includes a sign-out
 * conflated into the next account: the files have no owner to keep them for.
 */
internal fun sessionChange(
    owner: OutboxAccount?,
    previous: HostedSession,
    next: HostedSession,
): SessionChange {
    val old = owner ?: previous.filesOwner()
    if (old == null) {
        if (!previous.signedIn) return SessionChange.Keep
        if (!next.signedIn || next.filesOwner() != null) return SessionChange.Wipe(null)
        return SessionChange.Keep
    }
    val new = next.filesOwner()
    if (new == old) {
        return if (previous.signedIn && !next.signedIn) SessionChange.Expired else SessionChange.Keep
    }
    if (new == null || new.url != old.url) return SessionChange.Wipe(old)
    return SessionChange.OtherUser(old)
}

internal data class FileCleanup(
    val accounts: List<OutboxAccount>,
    /** `hosted/` is removed. A wipe is not downgraded to a social-only delete. */
    val wipeHosted: Boolean,
    /** Social files go and the downloads stay. False when [wipeHosted] already removes them. */
    val clearSocial: Boolean,
)

/** The disk owner and the account that just changed are both cleared. */
internal fun fileCleanup(sessionDelta: SessionChange, diskDelta: SessionChange): FileCleanup {
    val accounts = mutableListOf<OutboxAccount>()
    var wipeHosted = false
    var clearSocial = false
    fun absorb(change: SessionChange) {
        when (change) {
            is SessionChange.OtherUser -> {
                if (change.old !in accounts) accounts.add(change.old)
                clearSocial = true
            }
            is SessionChange.Wipe -> {
                change.old?.let { if (it !in accounts) accounts.add(it) }
                wipeHosted = true
            }
            SessionChange.Keep, SessionChange.Expired -> Unit
        }
    }
    absorb(diskDelta)
    absorb(sessionDelta)
    if (wipeHosted) clearSocial = false
    return FileCleanup(accounts, wipeHosted, clearSocial)
}

/** A 401 for the session still on screen expires it. The error still propagates. */
internal fun <T> HostedSessionStore.expireOn401(session: HostedSession, block: () -> T): T {
    return try {
        block()
    } catch (err: HostedException) {
        val token = session.token
        if (err.code == 401 && token != null) expire(token)
        throw err
    }
}
