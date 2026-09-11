package com.mewo.reader.ui.profile

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.ProfileReply
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class ProfileUiState(
    val handle: String,
    val replies: List<ProfileReply> = emptyList(),
    val likes: Map<String, Set<String>> = emptyMap(),
    val comments: Map<String, CommentIndex> = emptyMap(),
    val loading: Boolean = true,
    val error: String? = null,
)

class ProfileViewModel(
    private val store: LibraryStore,
    handle: String,
) : ViewModel() {
    private val _state = MutableStateFlow(ProfileUiState(handle = handle))
    val state: StateFlow<ProfileUiState> = _state.asStateFlow()

    fun refresh() {
        val handle = _state.value.handle
        viewModelScope.launch {
            _state.update { it.copy(loading = it.replies.isEmpty(), error = null) }
            runCatching {
                val replies = store.profileReplies(handle)
                val bookIds = replies.map { it.book.id }.distinct()
                val likes = bookIds.associateWith { id ->
                    runCatching { store.likes(id) }.getOrDefault(emptySet())
                }
                val comments = bookIds.associateWith { id ->
                    runCatching { store.commentIndex(id) }.getOrDefault(CommentIndex())
                }
                Triple(replies, likes, comments)
            }
                .onSuccess { (replies, likes, comments) ->
                    _state.update {
                        it.copy(
                            replies = replies,
                            likes = likes,
                            comments = comments,
                            loading = false,
                            error = null,
                        )
                    }
                }
                .onFailure { err ->
                    _state.update {
                        it.copy(
                            loading = false,
                            error = err.message ?: "Could not load this profile.",
                        )
                    }
                }
        }
    }

    fun toggleLike(bookId: String, postId: String) {
        viewModelScope.launch {
            runCatching { store.toggleLike(bookId, postId) }
                .onSuccess { ids ->
                    _state.update { it.copy(likes = it.likes + (bookId to ids)) }
                }
        }
    }

    fun onCommentsChanged(bookId: String, postId: String, comments: List<PostComment>) {
        _state.update { now ->
            val current = now.comments[bookId] ?: CommentIndex()
            val nextReplies = now.replies.filterNot { hit ->
                hit.book.id == bookId &&
                    hit.post.id == postId &&
                    comments.none { it.id == hit.comment.id }
            }
            now.copy(
                replies = nextReplies,
                comments = now.comments + (bookId to current.afterThread(postId, comments)),
            )
        }
    }

    companion object {
        fun factory(store: LibraryStore, handle: String) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return ProfileViewModel(store, handle) as T
            }
        }
    }
}
