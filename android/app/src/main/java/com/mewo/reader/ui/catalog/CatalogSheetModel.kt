package com.mewo.reader.ui.catalog

import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogException
import com.mewo.reader.data.CatalogStore
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.data.RequestDetail
import com.mewo.reader.data.RequestPhase
import com.mewo.reader.data.WorkDetail
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class CatalogSheetState(
    val work: CatalogWork,
    val detail: WorkDetail? = null,
    val fallback: BookRequest? = null,
    val loading: Boolean = true,
    val loadError: String? = null,
    val acting: Boolean = false,
    val notice: String? = null,
    val descriptionExpanded: Boolean = false,
)

val CatalogSheetState.status: SheetStatus
    get() = sheetStatus(work, detail, fallback)

val CatalogSheetState.alsoRequested: String?
    get() {
        val names = detail?.status?.otherRequests?.map { it.requester }.orEmpty()
        if (names.isEmpty()) return null
        return formatAlsoRequested(names)
    }

val CatalogSheetState.about: CatalogAbout?
    get() {
        val info = detail?.info ?: return null
        val subjects = subjectLine(info.subjects).takeIf { it.isNotEmpty() }
        val stats = workStats(info).takeIf { it.isNotEmpty() }
        return CatalogAbout(
            description = info.description?.takeIf { it.isNotBlank() },
            series = info.series,
            awards = info.awards,
            subjects = subjects,
            stats = stats,
            openLibraryUrl = "https://openlibrary.org${info.canonicalKey}",
        )
    }

/**
 * One open work. Polls every [CATALOG_POLL_MS] while the phase is live.
 * An action bumps [generation] so a pull that started earlier cannot land.
 * Cancel flips the row to canceled before the server answers.
 * [onImported] runs once, and only when a request becomes imported while
 * this sheet is open — a book that was already on the shelf does not reload it.
 */
class CatalogSheetModel(
    val work: CatalogWork,
    request: BookRequest?,
    private val store: CatalogStore,
    private val scope: CoroutineScope,
    private val onRequest: (BookRequest?) -> Unit,
    private val onImported: suspend () -> Unit,
) {
    private val _state = MutableStateFlow(CatalogSheetState(work = work, fallback = request))
    val state: StateFlow<CatalogSheetState> = _state.asStateFlow()

    private var generation = 0
    private var job: Job? = null
    private var sawDetail = false
    private var importedShelf = false

    fun start() = reload()

    fun stop() {
        generation += 1
        job?.cancel()
        job = null
    }

    fun reload() {
        val gen = ++generation
        job?.cancel()
        _state.update { it.copy(loading = it.detail == null, loadError = null) }
        job = scope.launch { runPull(gen) }
    }

    fun toggleDescription() {
        _state.update { it.copy(descriptionExpanded = !it.descriptionExpanded) }
    }

    fun request() {
        if (_state.value.acting || atCap()) return
        act { store.request(work) }
    }

    fun retry() {
        val id = currentRequest()?.id ?: return
        if (_state.value.acting || atCap()) return
        act { store.retry(id) }
    }

    fun cancel() {
        val detailRequest = _state.value.detail?.status?.request
        val current = detailRequest?.asBookRequest() ?: _state.value.fallback ?: return
        if (_state.value.acting) return
        val gen = ++generation
        job?.cancel()
        val canceled = current.copy(state = "canceled", error = null)
        _state.update { state ->
            val detail = state.detail
            val request = detail?.status?.request
            if (detail != null && request != null) {
                state.copy(
                    detail = detail.copy(
                        status = detail.status.copy(
                            request = request.copy(
                                state = "canceled",
                                error = null,
                                phase = RequestPhase.Canceled.wire,
                            ),
                        ),
                    ),
                    acting = true,
                    notice = null,
                )
            } else {
                state.copy(fallback = canceled, acting = true, notice = null)
            }
        }
        onRequest(canceled)
        job = scope.launch {
            try {
                store.cancel(current.id)
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                if (gen == generation) _state.update { it.copy(notice = error.message) }
            } finally {
                if (gen == generation) _state.update { it.copy(acting = false) }
            }
            if (gen == generation) runPull(gen)
        }
    }

    private fun act(block: suspend () -> BookRequest) {
        val gen = ++generation
        job?.cancel()
        job = scope.launch {
            _state.update { it.copy(acting = true, notice = null) }
            try {
                val next = block()
                if (gen != generation) return@launch
                onRequest(next)
                merge(next)
                if (next.state == "imported" && !importedShelf) {
                    importedShelf = true
                    onImported()
                }
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                if (gen == generation) _state.update { it.copy(notice = error.message) }
            } finally {
                if (gen == generation) _state.update { it.copy(acting = false) }
            }
            if (gen == generation) runPull(gen)
        }
    }

    private suspend fun runPull(gen: Int) {
        while (gen == generation) {
            try {
                val detail = store.work(work.workKey)
                if (gen != generation) return
                apply(detail)
                val phase = detail.status.request?.let { requestPhase(it) }
                if (phase?.live != true) return
            } catch (error: CancellationException) {
                throw error
            } catch (error: CatalogException) {
                if (gen != generation) return
                if (stopFor(error)) return
                delay(CATALOG_POLL_MS)
                continue
            } catch (error: Exception) {
                if (gen != generation) return
                noteLoadError(error.message)
                delay(CATALOG_POLL_MS)
                continue
            }
            delay(CATALOG_POLL_MS)
        }
    }

    private fun stopFor(error: CatalogException): Boolean {
        val stop = error.reason == CatalogException.Reason.Unauthorized ||
            error.reason == CatalogException.Reason.Off ||
            error.reason == CatalogException.Reason.NotFound
        if (_state.value.detail == null) noteLoadError(error.message)
        return stop
    }

    private fun noteLoadError(message: String?) {
        if (_state.value.detail != null) return
        _state.update {
            it.copy(
                loading = false,
                loadError = message ?: "Couldn't load details from Open Library.",
            )
        }
    }

    private fun apply(detail: WorkDetail) {
        val opening = !sawDetail
        val previous = _state.value.detail?.status?.request
        sawDetail = true
        _state.update { it.copy(detail = detail, loading = false, loadError = null) }
        val next = detail.status.request
        if (!sameRequest(previous, next)) onRequest(next?.asBookRequest())
        val phase = next?.phase
        if (phase != RequestPhase.Imported.wire) return
        if (opening) {
            importedShelf = true
            return
        }
        if (!importedShelf) {
            importedShelf = true
            scope.launch { onImported() }
        }
    }

    private fun merge(next: BookRequest) {
        _state.update { state ->
            val detail = state.detail ?: return@update state
            val request = detail.status.request ?: return@update state
            val merged = request.copy(
                id = next.id,
                workKey = next.workKey,
                title = next.title,
                author = next.author,
                year = next.year,
                coverId = next.coverId,
                state = next.state,
                progress = next.progress,
                error = next.error,
                bookId = next.bookId,
                requester = next.requester,
                createdAt = next.createdAt,
                updatedAt = next.updatedAt,
                follows = next.follows,
                phase = requestPhase(next).wire,
            )
            state.copy(detail = detail.copy(status = detail.status.copy(request = merged)))
        }
    }

    private fun currentRequest(): BookRequest? {
        return _state.value.detail?.status?.request?.asBookRequest() ?: _state.value.fallback
    }

    private fun atCap(): Boolean {
        val status = _state.value.detail?.status ?: return false
        return status.active >= status.cap
    }

    private fun sameRequest(left: RequestDetail?, right: RequestDetail?): Boolean {
        if (left == null || right == null) return left == right
        return left.id == right.id &&
            left.state == right.state &&
            left.progress == right.progress &&
            left.error == right.error &&
            left.bookId == right.bookId &&
            left.phase == right.phase
    }
}

private fun formatAlsoRequested(names: List<String>): String = alsoRequested(names)
