package com.mewo.reader.ui.reader

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class ReaderUiState(
    val book: BookRecord? = null,
    val posts: List<FeedPost> = emptyList(),
    val likes: Set<String> = emptySet(),
    val comments: CommentIndex = CommentIndex(),
    val startIndex: Int = 0,
    val loading: Boolean = true,
    val error: String? = null,
)

class ReaderViewModel(
    private val store: LibraryStore,
    private val bookId: String,
    private val focusPostId: String? = null,
) : ViewModel() {
    private val _state = MutableStateFlow(ReaderUiState())
    val state: StateFlow<ReaderUiState> = _state.asStateFlow()

    init {
        viewModelScope.launch {
            runCatching {
                store.book(bookId)
                    ?: store.load().let { store.book(bookId) }
                    ?: error("That book is gone.")
                val posts = store.feed(bookId)
                val book = store.book(bookId) ?: error("That book is gone.")
                val likes = store.likes(bookId)
                val comments = store.commentIndex(bookId)
                val focus = focusPostId?.let { id -> posts.indexOfFirst { it.id == id } }
                    ?.takeIf { it >= 0 }
                val start = focus ?: book.progressIndex.coerceIn(0, posts.lastIndex.coerceAtLeast(0))
                _state.value = ReaderUiState(
                    book = book,
                    posts = posts,
                    likes = likes,
                    comments = comments,
                    startIndex = start,
                    loading = false,
                )
            }.onFailure { err ->
                _state.update {
                    it.copy(loading = false, error = err.message ?: "Could not open this book.")
                }
            }
        }
    }

    fun toggleLike(postId: String) {
        viewModelScope.launch {
            val next = store.toggleLike(bookId, postId)
            _state.update { it.copy(likes = next) }
        }
    }

    fun onCommentsChanged(postId: String, comments: List<PostComment>) {
        _state.update { it.copy(comments = it.comments.afterThread(postId, comments)) }
    }

    fun saveProgress(index: Int) {
        viewModelScope.launch {
            store.saveProgress(bookId, index)
        }
    }

    companion object {
        fun factory(
            store: LibraryStore,
            bookId: String,
            focusPostId: String? = null,
        ) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return ReaderViewModel(store, bookId, focusPostId) as T
            }
        }
    }
}
