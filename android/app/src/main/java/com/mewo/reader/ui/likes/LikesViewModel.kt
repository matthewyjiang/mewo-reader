package com.mewo.reader.ui.likes

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostHit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class LikesUiState(
    val hits: List<PostHit> = emptyList(),
    val loading: Boolean = true,
)

class LikesViewModel(
    private val store: LibraryStore,
) : ViewModel() {
    private val _state = MutableStateFlow(LikesUiState())
    val state: StateFlow<LikesUiState> = _state.asStateFlow()

    fun refresh() {
        viewModelScope.launch {
            _state.update { it.copy(loading = it.hits.isEmpty()) }
            runCatching { store.likedPosts() }
                .onSuccess { hits ->
                    _state.update { it.copy(hits = hits, loading = false) }
                }
                .onFailure {
                    _state.update { it.copy(hits = emptyList(), loading = false) }
                }
        }
    }

    fun unlike(bookId: String, postId: String) {
        viewModelScope.launch {
            runCatching { store.toggleLike(bookId, postId) }
                .onSuccess {
                    _state.update {
                        it.copy(
                            hits = it.hits.filterNot { hit ->
                                hit.post.id == postId && hit.book.id == bookId
                            },
                        )
                    }
                }
        }
    }

    companion object {
        fun factory(store: LibraryStore) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return LikesViewModel(store) as T
            }
        }
    }
}
