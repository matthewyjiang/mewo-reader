package com.mewo.reader.ui.search

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.SearchResult
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class SearchUiState(
    val query: String = "",
    val result: SearchResult = SearchResult(),
    val likes: Map<String, Set<String>> = emptyMap(),
)

class SearchViewModel(
    private val store: LibraryStore,
) : ViewModel() {
    private val _state = MutableStateFlow(SearchUiState())
    val state: StateFlow<SearchUiState> = _state.asStateFlow()

    fun setQuery(query: String) {
        _state.update { it.copy(query = query) }
        viewModelScope.launch {
            val hits = store.search(query)
            val likes = hits.posts.map { it.book.id }.distinct()
                .associateWith { store.likes(it) }
            _state.update { now ->
                if (now.query == query) now.copy(result = hits, likes = now.likes + likes) else now
            }
        }
    }

    fun toggleLike(bookId: String, postId: String) {
        viewModelScope.launch {
            val next = store.toggleLike(bookId, postId)
            _state.update { it.copy(likes = it.likes + (bookId to next)) }
        }
    }

    companion object {
        fun factory(store: LibraryStore) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return SearchViewModel(store) as T
            }
        }
    }
}
