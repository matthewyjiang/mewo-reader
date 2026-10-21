package com.mewo.reader.ui.search

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.CatalogAccess
import com.mewo.reader.data.CatalogStore
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.DisabledCatalogStore
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.SearchKind
import com.mewo.reader.data.SearchPage
import com.mewo.reader.data.SearchResult
import com.mewo.reader.data.cursor
import com.mewo.reader.data.kind
import com.mewo.reader.ui.catalog.CatalogFeed
import com.mewo.reader.ui.catalog.RowOutcome
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class SearchUiState(
    val search: SearchState = SearchState(),
    val result: SearchResult = SearchResult(),
    val likes: Map<String, Set<String>> = emptyMap(),
    val comments: Map<String, CommentIndex> = emptyMap(),
) {
    val query: String get() = search.query
}

class SearchViewModel(
    private val store: LibraryStore,
    private val memory: SearchKindMemory = MemorySearchKind(),
    private val catalogStore: CatalogStore = DisabledCatalogStore(),
) : ViewModel() {
    private val initialKind = rememberKind(memory.read(), store.hostedSocial, catalogStore.access.value)
    private val _state = MutableStateFlow(
        SearchUiState(search = SearchState(kind = initialKind, session = "local")),
    )
    val state: StateFlow<SearchUiState> = _state.asStateFlow()

    val catalog = CatalogFeed(
        store = catalogStore,
        scope = viewModelScope,
        onImported = { reloadLibrary { store.load() } },
    )
    private var restoredCatalog = false

    private val social = BookSocial(store, viewModelScope) { likes, comments ->
        _state.update { it.copy(likes = likes, comments = comments) }
    }

    private var searchJob: Job? = null
    private var generation = 0L
    private var session: String? = "local"
    private var pendingCursor: String? = null

    init {
        viewModelScope.launch {
            catalogStore.access.collect { access ->
                when (access) {
                    CatalogAccess.Unknown -> Unit
                    CatalogAccess.Off -> {
                        if (_state.value.search.kind == SearchKind.Catalog) {
                            val fallback = if (store.hostedSocial) SearchKind.Accounts else SearchKind.Books
                            setKind(fallback)
                        }
                    }
                    CatalogAccess.On -> {
                        if (!restoredCatalog) {
                            restoredCatalog = true
                            if (memory.read() == SearchKind.Catalog &&
                                _state.value.search.kind != SearchKind.Catalog
                            ) {
                                setKind(SearchKind.Catalog)
                            }
                        }
                        if (_state.value.search.kind == SearchKind.Catalog) catalog.show()
                    }
                }
            }
        }
    }

    /**
     * Shows [query] right away. The request waits [SEARCH_DEBOUNCE_MS], and a
     * newer keystroke or tab cancels it. A response for another session or
     * query is dropped. The catalog tab never asks `/v1/search`.
     */
    fun actOnCatalog(work: CatalogWork, onOpenBook: (String) -> Unit) {
        viewModelScope.launch {
            when (val outcome = catalog.act(work)) {
                is RowOutcome.OpenBook -> onOpenBook(outcome.id)
                RowOutcome.Done -> Unit
            }
        }
    }

    fun setQuery(query: String) {
        catalog.setQuery(query)
        if (_state.value.search.kind == SearchKind.Catalog) {
            _state.update { it.copy(search = it.search.copy(query = query)) }
            return
        }
        val trimmed = query.trim()
        val same = trimmed == _state.value.query.trim()
        if (same) {
            _state.update { it.copy(search = it.search.copy(query = query)) }
            return
        }
        val gen = ++generation
        searchJob?.cancel()
        pendingCursor = null
        if (trimmed.isEmpty() || session == null) {
            _state.update {
                it.copy(search = it.search.copy(query = query, phase = SearchPhase.Idle), result = SearchResult())
            }
            return
        }
        val previous = visiblePage(_state.value.search)?.takeIf { it.kind() == _state.value.search.kind }
        _state.update {
            it.copy(search = it.search.copy(query = query, phase = SearchPhase.Loading(previous)))
        }
        arm(gen, trimmed, _state.value.search.kind, null)
    }

    fun setKind(kind: SearchKind) {
        if (!store.hostedSocial && kind == SearchKind.Accounts) return
        if (kind == SearchKind.Catalog && catalogStore.access.value != CatalogAccess.On) return
        if (kind == _state.value.search.kind) return
        memory.write(kind)
        val gen = ++generation
        searchJob?.cancel()
        pendingCursor = null
        if (kind == SearchKind.Catalog) {
            catalog.show()
            _state.update { it.copy(search = it.search.copy(kind = kind, phase = SearchPhase.Idle)) }
            return
        }
        catalog.hide()
        val asking = _state.value.query.isNotBlank() && session != null
        _state.update {
            it.copy(
                search = it.search.copy(
                    kind = kind,
                    phase = if (asking) SearchPhase.Loading(null) else SearchPhase.Idle,
                ),
            )
        }
        if (asking) arm(gen, _state.value.query.trim(), kind, null)
    }

    fun bindSession(id: String?) {
        catalog.bindSession(id)
        if (id == session) return
        session = id
        generation++
        searchJob?.cancel()
        pendingCursor = null
        _state.update {
            it.copy(
                search = it.search.copy(session = id, phase = SearchPhase.Idle),
                result = if (id == null) SearchResult() else it.result,
            )
        }
    }

    fun retry() {
        val phase = _state.value.search.phase as? SearchPhase.Failed ?: return
        val query = _state.value.query.trim()
        val id = session ?: return
        if (query.isEmpty()) return
        val gen = ++generation
        searchJob?.cancel()
        _state.update {
            it.copy(search = it.search.copy(phase = SearchPhase.Loading(phase.previous)))
        }
        val cursor = pendingCursor
        searchJob = viewModelScope.launch {
            run(gen, id, query, _state.value.search.kind, cursor)
        }
    }

    fun loadMore() {
        val phase = _state.value.search.phase as? SearchPhase.Loaded ?: return
        if (phase.more) return
        val cursor = phase.page.cursor() ?: return
        val query = _state.value.query.trim()
        val id = session ?: return
        if (query.isEmpty()) return
        val gen = ++generation
        searchJob?.cancel()
        pendingCursor = cursor
        _state.update { it.copy(search = it.search.copy(phase = phase.copy(more = true))) }
        searchJob = viewModelScope.launch {
            run(gen, id, query, _state.value.search.kind, cursor)
        }
    }

    fun refreshSocial() = social.markStale()

    fun onCommentsChanged(bookId: String, postId: String, comments: List<PostComment>) =
        social.onThreadChanged(bookId, postId, comments)

    fun toggleLike(bookId: String, postId: String) = social.toggleLike(bookId, postId)

    fun toggleRepost(bookId: String, postId: String) = social.toggleRepost(bookId, postId)

    private fun arm(gen: Long, query: String, kind: SearchKind, cursor: String?) {
        val requestSession = session ?: return
        pendingCursor = cursor
        searchJob = viewModelScope.launch {
            delay(SEARCH_DEBOUNCE_MS)
            run(gen, requestSession, query, kind, cursor)
        }
    }

    private suspend fun run(
        gen: Long,
        requestSession: String,
        query: String,
        kind: SearchKind,
        cursor: String?,
    ) {
        if (!current(gen, requestSession, query, kind)) return
        try {
            if (store.hostedSocial) {
                val page = store.searchPage(query, kind, cursor)
                if (!current(gen, requestSession, query, kind)) return
                if (page.kind() != kind) return
                val merged = if (cursor != null) append(visiblePage(_state.value.search), page) else page
                _state.update {
                    it.copy(search = it.search.copy(phase = SearchPhase.Loaded(merged, more = false)))
                }
                if (merged is SearchPage.Posts) social.show(merged.items.map { it.book.id }.distinct())
            } else {
                val hits = attempt { store.search(query) } ?: return
                if (!current(gen, requestSession, query, kind)) return
                _state.update {
                    it.copy(
                        result = hits,
                        search = it.search.copy(
                            phase = SearchPhase.Loaded(localPage(kind, hits), more = false),
                        ),
                    )
                }
                social.show(hits.posts.map { it.book.id }.distinct())
            }
        } catch (error: CancellationException) {
            throw error
        } catch (error: Exception) {
            if (!current(gen, requestSession, query, kind)) return
            _state.update {
                it.copy(
                    search = it.search.copy(
                        phase = SearchPhase.Failed(
                            error.message ?: "Could not search.",
                            visiblePage(it.search),
                        ),
                    ),
                )
            }
        }
    }

    private fun current(gen: Long, requestSession: String, query: String, kind: SearchKind): Boolean {
        return gen == generation && requestSession == session &&
            query == _state.value.query.trim() && kind == _state.value.search.kind
    }

    private fun append(current: SearchPage?, next: SearchPage): SearchPage {
        if (current == null || current.kind() != next.kind()) return next
        return when {
            current is SearchPage.Accounts && next is SearchPage.Accounts ->
                next.copy(items = current.items + next.items)
            current is SearchPage.Books && next is SearchPage.Books ->
                next.copy(items = current.items + next.items)
            current is SearchPage.Posts && next is SearchPage.Posts ->
                next.copy(items = current.items + next.items)
            else -> next
        }
    }

    companion object {
        fun factory(
            store: LibraryStore,
            memory: SearchKindMemory = MemorySearchKind(),
            catalog: CatalogStore = DisabledCatalogStore(),
        ) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return SearchViewModel(store, memory, catalog) as T
            }
        }
    }
}

private fun rememberKind(saved: SearchKind?, hosted: Boolean, access: CatalogAccess): SearchKind = when {
    saved == SearchKind.Accounts && !hosted -> SearchKind.Books
    saved == SearchKind.Catalog && !hosted -> SearchKind.Books
    saved == SearchKind.Catalog && access == CatalogAccess.Unknown ->
        if (hosted) SearchKind.Accounts else SearchKind.Books
    saved != null -> saved
    hosted -> SearchKind.Accounts
    else -> SearchKind.Books
}

/** A library reload must not keep a cancelled caller alive. Other failures stay on the sheet. */
internal suspend fun reloadLibrary(load: suspend () -> Unit) {
    try {
        load()
    } catch (error: CancellationException) {
        throw error
    } catch (_: Exception) {
    }
}

private fun localPage(kind: SearchKind, result: SearchResult): SearchPage = when (kind) {
    SearchKind.Books -> SearchPage.Books(result.books)
    SearchKind.Posts -> SearchPage.Posts(result.posts.map { com.mewo.reader.data.PostHitRecord(it.book, it.post) })
    SearchKind.Accounts -> SearchPage.Accounts()
    SearchKind.Catalog -> error("Catalog does not use /v1/search.")
}
