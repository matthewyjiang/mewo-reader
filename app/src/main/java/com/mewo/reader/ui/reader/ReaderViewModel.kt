package com.mewo.reader.ui.reader

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.LibraryRepository
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class ReaderUiState(
    val book: BookRecord? = null,
    val posts: List<FeedPost> = emptyList(),
    val likes: Set<String> = emptySet(),
    val startIndex: Int = 0,
    val loading: Boolean = true,
    val error: String? = null,
)

class ReaderViewModel(
    private val repository: LibraryRepository,
    private val bookId: String,
) : ViewModel() {
    private val _state = MutableStateFlow(ReaderUiState())
    val state: StateFlow<ReaderUiState> = _state.asStateFlow()

    init {
        viewModelScope.launch {
            runCatching {
                val book = repository.book(bookId)
                    ?: repository.load().let { repository.book(bookId) }
                    ?: error("That book is gone.")
                val posts = repository.feed(bookId)
                val likes = repository.likes(bookId)
                _state.value = ReaderUiState(
                    book = book,
                    posts = posts,
                    likes = likes,
                    startIndex = book.progressIndex.coerceIn(0, posts.lastIndex.coerceAtLeast(0)),
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
            val next = repository.toggleLike(bookId, postId)
            _state.update { it.copy(likes = next) }
        }
    }

    fun saveProgress(index: Int) {
        viewModelScope.launch {
            repository.saveProgress(bookId, index)
        }
    }

    companion object {
        fun factory(repository: LibraryRepository, bookId: String) =
            object : ViewModelProvider.Factory {
                @Suppress("UNCHECKED_CAST")
                override fun <T : ViewModel> create(modelClass: Class<T>): T {
                    return ReaderViewModel(repository, bookId) as T
                }
            }
    }
}
