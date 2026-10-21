package com.mewo.reader.ui.catalog

import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogAccess
import com.mewo.reader.data.CatalogPage
import com.mewo.reader.data.CatalogStore
import com.mewo.reader.data.CatalogWork
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

sealed class CatalogPhase {
    data object Idle : CatalogPhase()

    data class Loading(val previous: CatalogPage? = null) : CatalogPhase()

    data class Loaded(val page: CatalogPage, val more: Boolean = false) : CatalogPhase()

    /** The failure replaces the list. There is no stale page behind it. */
    data class Failed(val message: String) : CatalogPhase()
}

data class CatalogListState(
    val phase: CatalogPhase = CatalogPhase.Idle,
    val overlay: Map<String, BookRequest?> = emptyMap(),
    val sheet: CatalogSheetModel? = null,
    val notice: String? = null,
)

sealed class RowOutcome {
    data object Done : RowOutcome()
    data class OpenBook(val id: String) : RowOutcome()
}

private data class Memo(val page: CatalogPage, val pages: Int)

private const val MEMO_LIMIT = 8

/**
 * Open Library list for the Catalog tab. Trending (a blank query, or fewer
 * than [CATALOG_MIN_QUERY] Unicode scalars) is memoized per session. A memo
 * of a single page is refreshed in place; a longer one is left alone. A
 * failed load replaces whatever was on screen.
 *
 * Only this feed writes [CatalogListState.overlay]. The sheet reports
 * request changes back through [CatalogSheetModel].
 */
class CatalogFeed(
    private val store: CatalogStore,
    private val scope: CoroutineScope,
    private val onImported: suspend () -> Unit,
) {
    private val _state = MutableStateFlow(CatalogListState())
    val state: StateFlow<CatalogListState> = _state.asStateFlow()

    private var rawQuery = ""
    private var session: String? = null
    private var visible = false
    private var generation = 0L
    private var job: Job? = null
    private val memos = mutableMapOf<String, LinkedHashMap<String, Memo>>()

    fun setQuery(raw: String) {
        val previous = catalogQuery(rawQuery)
        val next = catalogQuery(raw)
        rawQuery = raw
        if (next == previous) return
        _state.value.sheet?.stop()
        _state.update { it.copy(overlay = emptyMap(), sheet = null, notice = null) }
        if (visible) publish()
    }

    fun show() {
        if (visible) {
            if (store.access.value == CatalogAccess.On && _state.value.phase is CatalogPhase.Idle) publish()
            return
        }
        visible = true
        if (store.access.value != CatalogAccess.On) return
        publish()
    }

    fun hide() {
        if (!visible) return
        visible = false
        generation += 1
        job?.cancel()
        job = null
    }

    fun bindSession(id: String?) {
        if (id == session) return
        session = id
        generation += 1
        job?.cancel()
        _state.value.sheet?.stop()
        _state.value = CatalogListState()
        if (visible && id != null && store.access.value == CatalogAccess.On) publish()
    }

    fun loadMore() {
        val phase = _state.value.phase as? CatalogPhase.Loaded ?: return
        if (phase.more || phase.page.nextPage == null || !visible) return
        val id = session ?: return
        val asked = catalogQuery(rawQuery)
        val nextPage = phase.page.nextPage
        val gen = ++generation
        job?.cancel()
        _state.update { it.copy(phase = phase.copy(more = true)) }
        job = scope.launch { fetch(gen, id, asked, nextPage, append = true) }
    }

    fun retry() {
        if (_state.value.phase !is CatalogPhase.Failed || !visible) return
        val id = session ?: return
        val asked = catalogQuery(rawQuery)
        val gen = ++generation
        job?.cancel()
        _state.update { it.copy(phase = CatalogPhase.Loading()) }
        job = scope.launch { fetch(gen, id, asked, page = 1, append = false) }
    }

    fun dismissNotice() {
        _state.update { it.copy(notice = null) }
    }

    fun requestFor(work: CatalogWork): BookRequest? {
        val overlay = _state.value.overlay
        return if (work.workKey in overlay) overlay[work.workKey] else work.request
    }

    suspend fun act(work: CatalogWork): RowOutcome {
        val current = requestFor(work)
        return when (rowAction(current, work.onServer)) {
            RowAction.Pending -> RowOutcome.Done
            RowAction.Open -> {
                val id = current?.bookId ?: work.onServer?.bookId
                if (id.isNullOrBlank()) RowOutcome.Done else RowOutcome.OpenBook(id)
            }
            RowAction.Request -> submit(work.workKey) { store.request(work) }
            RowAction.Again -> {
                // Retry accepts only failed or canceled. An import with no book starts a new request.
                val id = current?.takeIf { it.state == "canceled" }?.id
                if (id != null) {
                    submit(work.workKey) { store.retry(id) }
                } else {
                    submit(work.workKey) { store.request(work) }
                }
            }
            RowAction.Retry -> {
                val id = current?.id ?: return RowOutcome.Done
                submit(work.workKey) { store.retry(id) }
            }
        }
    }

    fun open(work: CatalogWork) {
        _state.value.sheet?.stop()
        val model = CatalogSheetModel(
            work = work,
            request = requestFor(work),
            store = store,
            scope = scope,
            onRequest = { next -> setOverlay(work.workKey, next) },
            onImported = onImported,
        )
        _state.update { it.copy(sheet = model) }
        model.start()
    }

    fun close() {
        _state.value.sheet?.stop()
        _state.update { it.copy(sheet = null) }
    }

    private suspend fun submit(workKey: String, block: suspend () -> BookRequest): RowOutcome {
        try {
            val next = block()
            setOverlay(workKey, next)
            if (next.state == "imported") onImported()
        } catch (error: CancellationException) {
            throw error
        } catch (error: Exception) {
            _state.update { it.copy(notice = error.message ?: "Could not request that book.") }
        }
        return RowOutcome.Done
    }

    private fun setOverlay(workKey: String, request: BookRequest?) {
        _state.update { it.copy(overlay = it.overlay + (workKey to request)) }
    }

    private fun publish() {
        if (!visible) return
        val id = session ?: return
        if (store.access.value != CatalogAccess.On) return
        val asked = catalogQuery(rawQuery)
        val gen = ++generation
        job?.cancel()
        val hit = recall(id, asked.memoKey())
        if (hit != null && hit.pages > 1) {
            _state.update { it.copy(phase = CatalogPhase.Loaded(hit.page, more = false)) }
            return
        }
        val quiet = hit != null
        if (hit != null) {
            _state.update { it.copy(phase = CatalogPhase.Loaded(hit.page, more = false)) }
        } else {
            val previous = if (asked is CatalogQuery.Search) shownPage() else null
            _state.update { it.copy(phase = CatalogPhase.Loading(previous)) }
        }
        val wait = when {
            quiet -> 0L
            asked is CatalogQuery.Browse -> 0L
            else -> CATALOG_DEBOUNCE_MS
        }
        job = scope.launch {
            if (wait > 0) delay(wait)
            fetch(gen, id, asked, page = 1, append = false)
        }
    }

    private suspend fun fetch(
        gen: Long,
        id: String,
        asked: CatalogQuery,
        page: Int,
        append: Boolean,
    ) {
        if (!current(gen, id, asked)) return
        try {
            val next = store.page(asked.wireQuery(), page)
            if (!current(gen, id, asked)) return
            val merged = if (!append) {
                next
            } else {
                val existing = (_state.value.phase as? CatalogPhase.Loaded)?.page
                if (existing == null) next
                else existing.copy(items = existing.items + next.items, nextPage = next.nextPage)
            }
            val prior = if (append) recall(id, asked.memoKey())?.pages ?: 1 else 0
            remember(id, asked.memoKey(), merged, if (append) prior + 1 else 1)
            _state.update { it.copy(phase = CatalogPhase.Loaded(merged, more = false)) }
        } catch (error: CancellationException) {
            throw error
        } catch (error: Exception) {
            if (!current(gen, id, asked)) return
            memos[id]?.remove(asked.memoKey())
            _state.update {
                it.copy(phase = CatalogPhase.Failed(error.message ?: "Could not load the catalog."))
            }
        }
    }

    private fun shownPage(): CatalogPage? = when (val phase = _state.value.phase) {
        is CatalogPhase.Loaded -> phase.page
        is CatalogPhase.Loading -> phase.previous
        else -> null
    }

    private fun current(gen: Long, id: String, asked: CatalogQuery): Boolean {
        return gen == generation && id == session && visible && catalogQuery(rawQuery) == asked
    }

    private fun recall(id: String, key: String): Memo? = memos[id]?.get(key)

    private fun remember(id: String, key: String, page: CatalogPage, pages: Int) {
        val memo = memos.getOrPut(id) { LinkedHashMap() }
        memo.remove(key)
        memo[key] = Memo(page, pages)
        while (memo.size > MEMO_LIMIT) {
            val oldest = memo.keys.firstOrNull() ?: break
            memo.remove(oldest)
        }
    }
}

private fun CatalogQuery.memoKey(): String = when (this) {
    CatalogQuery.Browse -> ""
    is CatalogQuery.Search -> q
}

private fun CatalogQuery.wireQuery(): String = memoKey()
