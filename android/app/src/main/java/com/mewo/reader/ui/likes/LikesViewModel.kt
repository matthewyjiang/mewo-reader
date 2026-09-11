package com.mewo.reader.ui.likes

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.PostHit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class LikesUiState(
    val hits: List<PostHit> = emptyList(),
    val comments: Map<String, CommentIndex> = emptyMap(),
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
            runCatching {
                val hits = store.likedPosts()
                val comments = hits.map { it.book.id }.distinct().associateWith { id ->
                    runCatching { store.commentIndex(id) }.getOrDefault(CommentIndex())
                }
                hits to comments
            }
                .onSuccess { (hits, comments) ->
                    _state.update { it.copy(hits = hits, comments = comments, loading = false) }
                }
                .onFailure {
                    _state.update { it.copy(hits = emptyList(), loading = false) }
                }
        }
    }

    fun onCommentsChanged(bookId: String, postId: String, comments: List<PostComment>) {
        _state.update { now ->
            val current = now.comments[bookId] ?: CommentIndex()
            now.copy(comments = now.comments + (bookId to current.afterThread(postId, comments)))
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
