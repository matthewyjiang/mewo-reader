package com.mewo.reader.data

import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.mewo.reader.ui.catalog.catalogErrorText
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.IOException

interface CatalogStore {
    val access: StateFlow<CatalogAccess>

    suspend fun refreshAccess()

    suspend fun page(query: String, page: Int): CatalogPage

    suspend fun work(workKey: String): WorkDetail

    suspend fun request(work: CatalogWork): BookRequest

    suspend fun cancel(id: String)

    suspend fun retry(id: String): BookRequest
}

/**
 * Catalog follows the signed-in hosted session. Local and signed-out are off.
 * `/v1/me` runs when the session changes and when the process resumes.
 * A 403 turns the catalog off. Every failure leaves as a [CatalogException]
 * whose message is already human.
 */
class HostedCatalogStore(
    private val sessions: HostedSessionStore,
    private val client: HostedClient,
    private val backend: StateFlow<BackendKind>,
    private val scope: CoroutineScope,
) : CatalogStore, DefaultLifecycleObserver {
    private val _access = MutableStateFlow(CatalogAccess.Unknown)
    override val access: StateFlow<CatalogAccess> = _access.asStateFlow()
    private var accessGeneration = 0

    init {
        ProcessLifecycleOwner.get().lifecycle.addObserver(this)
        scope.launch {
            var previous: Pair<String?, BackendKind>? = null
            combine(sessions.session, backend) { session, kind ->
                session to kind
            }.collect { (session, kind) ->
                val key = session.token to kind
                if (key == previous) return@collect
                previous = key
                refreshAccess()
            }
        }
    }

    override fun onResume(owner: LifecycleOwner) {
        scope.launch { refreshAccess() }
    }

    override suspend fun refreshAccess() {
        val generation = ++accessGeneration
        val session = sessions.session.value
        val kind = backend.value
        if (kind != BackendKind.Hosted || !session.signedIn) {
            if (generation == accessGeneration) _access.value = CatalogAccess.Off
            return
        }
        try {
            val me = withContext(Dispatchers.IO) {
                sessions.expireOn401(session) { client.me(session) }
            }
            if (generation != accessGeneration) return
            if (sessions.session.value.token != session.token) return
            _access.value = if (me.canRequest) CatalogAccess.On else CatalogAccess.Off
        } catch (err: HostedException) {
            if (generation != accessGeneration) return
            if (err.code == 403 || err.code == 401) _access.value = CatalogAccess.Off
        } catch (_: IOException) {
            // Leave the last answer. A blip should not hide the tab.
        }
    }

    override suspend fun page(query: String, page: Int): CatalogPage = call { session ->
        client.catalog(session, query, page)
    }

    override suspend fun work(workKey: String): WorkDetail = call { session ->
        client.catalogWork(session, catalogOlid(workKey))
    }

    override suspend fun request(work: CatalogWork): BookRequest = call { session ->
        client.requestBook(session, work)
    }

    override suspend fun cancel(id: String) {
        call<Unit> { session -> client.cancelRequest(session, id) }
    }

    override suspend fun retry(id: String): BookRequest = call { session ->
        client.retryRequest(session, id)
    }

    private suspend fun <T> call(block: (HostedSession) -> T): T {
        val session = sessions.session.value
        if (backend.value != BackendKind.Hosted || !session.signedIn) {
            _access.value = CatalogAccess.Off
            throw CatalogException(CatalogException.Reason.Off, catalogErrorText(403, "Requests are off.", false))
        }
        try {
            return withContext(Dispatchers.IO) {
                sessions.expireOn401(session) { block(session) }
            }
        } catch (err: CatalogException) {
            throw err
        } catch (err: HostedException) {
            if (err.code == 403) _access.value = CatalogAccess.Off
            throw err.toCatalog()
        } catch (_: IOException) {
            throw CatalogException(
                CatalogException.Reason.Network,
                catalogErrorText(0, null, network = true),
            )
        }
    }
}

private fun HostedException.toCatalog(): CatalogException {
    val reason = when (code) {
        401 -> CatalogException.Reason.Unauthorized
        403 -> CatalogException.Reason.Off
        404 -> CatalogException.Reason.NotFound
        0 -> CatalogException.Reason.Network
        else -> CatalogException.Reason.Failed
    }
    val network = code == 0 && (message.isNullOrBlank() || message?.contains("connection", ignoreCase = true) == true)
    return CatalogException(reason, catalogErrorText(code, message, network))
}

/** Catalog stays off. Search tests that never open it use this. */
class DisabledCatalogStore : CatalogStore {
    override val access: StateFlow<CatalogAccess> = MutableStateFlow(CatalogAccess.Off).asStateFlow()

    override suspend fun refreshAccess() = Unit

    override suspend fun page(query: String, page: Int): CatalogPage = off()

    override suspend fun work(workKey: String): WorkDetail = off()

    override suspend fun request(work: CatalogWork): BookRequest = off()

    override suspend fun cancel(id: String) { off<Unit>() }

    override suspend fun retry(id: String): BookRequest = off()

    private fun <T> off(): T {
        throw CatalogException(CatalogException.Reason.Off, catalogErrorText(403, "Requests are off.", false))
    }
}
