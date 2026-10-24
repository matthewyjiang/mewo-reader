package com.mewo.reader.ui.reader

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.commitRepost
import kotlinx.coroutines.CancellationException
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
            val opened = try {
                store.book(bookId)
                    ?: store.load().let { store.book(bookId) }
                    ?: error("That book is gone.")
                val posts = store.feed(bookId)
                val book = store.book(bookId) ?: error("That book is gone.")
                posts to book
            } catch (e: CancellationException) {
                throw e
            } catch (err: Exception) {
                _state.update {
                    it.copy(loading = false, error = err.message ?: "Could not open this book.")
                }
                return@launch
            }
            val (posts, book) = opened
            // Likes and replies are not what makes the book readable.
            val likes = socialOrNull { store.likes(bookId) } ?: emptySet()
            val comments = socialOrNull { store.commentIndex(bookId) } ?: CommentIndex()
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
        }
    }

    fun toggleLike(postId: String) {
        val before = _state.value.likes
        val optimistic = if (postId in before) before - postId else before + postId
        _state.update { it.copy(likes = optimistic) }
        viewModelScope.launch {
            try {
                val next = store.toggleLike(bookId, postId)
                _state.update { it.copy(likes = next) }
            } catch (e: CancellationException) {
                _state.update { if (it.likes == optimistic) it.copy(likes = before) else it }
                throw e
            } catch (e: Exception) {
                _state.update { if (it.likes == optimistic) it.copy(likes = before) else it }
            }
        }
    }

    fun toggleRepost(postId: String) {
        val before = _state.value.comments
        viewModelScope.launch {
            runCatching {
                commitRepost(
                    before,
                    postId,
                    { on -> store.setRepost(bookId, postId, on) },
                    { _state.value.comments },
                    { next -> _state.update { it.copy(comments = next) } },
                )
            }
        }
    }

    fun onCommentsChanged(postId: String, comments: List<PostComment>) {
        _state.update { it.copy(comments = it.comments.afterThread(postId, comments)) }
    }

    fun saveProgress(index: Int) {
        viewModelScope.launch {
            try {
                store.saveProgress(bookId, index)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // The reader stays on this post. The shelf percent is whatever the server kept.
            }
        }
    }

    private suspend fun <T> socialOrNull(block: suspend () -> T): T? = try {
        block()
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        null
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
