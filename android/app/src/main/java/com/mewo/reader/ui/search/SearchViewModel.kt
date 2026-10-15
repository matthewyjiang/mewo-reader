package com.mewo.reader.ui.search

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.SearchResult
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class SearchUiState(
    val query: String = "",
    val result: SearchResult = SearchResult(),
    val likes: Map<String, Set<String>> = emptyMap(),
    val comments: Map<String, CommentIndex> = emptyMap(),
)

class SearchViewModel(
    private val store: LibraryStore,
) : ViewModel() {
    private val _state = MutableStateFlow(SearchUiState())
    val state: StateFlow<SearchUiState> = _state.asStateFlow()

    // Likes and comment counts belong to books, not queries, so they outlive
    // every keystroke. The search job below never touches them.
    private val social = BookSocial(store, viewModelScope) { likes, comments ->
        _state.update { it.copy(likes = likes, comments = comments) }
    }

    // The one search that may still publish. A keystroke cancels it and bumps
    // [generation], so a stale run that ignores cancellation still cannot
    // publish, even when the text goes back to an older value (a, ab, a).
    private var searchJob: Job? = null
    private var generation = 0L

    /**
     * Shows [query] right away, then publishes its hits as soon as the scan
     * ends. Likes and comment counts for the hit books load after that, so a
     * slow network never holds back the results.
     */
    fun setQuery(query: String) {
        val gen = ++generation
        _state.update { it.copy(query = query) }
        searchJob?.cancel()
        searchJob = viewModelScope.launch {
            val hits = attempt { store.search(query) } ?: return@launch
            if (gen != generation) return@launch
            _state.update { it.copy(result = hits) }
            social.show(hits.posts.map { it.book.id }.distinct())
        }
    }

    /** Reloads likes and comment counts for the shown books, e.g. when the screen comes back. */
    fun refreshSocial() = social.markStale()

    fun onCommentsChanged(bookId: String, postId: String, comments: List<PostComment>) =
        social.onThreadChanged(bookId, postId, comments)

    fun toggleLike(bookId: String, postId: String) = social.toggleLike(bookId, postId)

    companion object {
        fun factory(store: LibraryStore) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return SearchViewModel(store) as T
            }
        }
    }
}
