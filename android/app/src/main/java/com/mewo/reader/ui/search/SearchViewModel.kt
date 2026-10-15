package com.mewo.reader.ui.search

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.SearchResult
import kotlinx.coroutines.CancellationException
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

    // The one search that may still publish. A keystroke cancels it and bumps
    // [generation], so a stale run that ignores cancellation still cannot
    // publish, even when the text goes back to an older value (a, ab, a).
    private var searchJob: Job? = null
    private var generation = 0L

    // Bumped per book each time a like toggle publishes. A likes load that
    // started before the bump is older than the toggle, so it must not publish.
    private val likeRevision = mutableMapOf<String, Long>()

    // Threads edited since the current query started, per book and post. A
    // comment index that lands after an edit gets the edits laid back on top.
    private val commentEdits = mutableMapOf<String, MutableMap<String, List<PostComment>>>()

    /**
     * Shows [query] right away, then publishes its hits as soon as the scan
     * ends. Likes and comment counts then load one book at a time, so a slow
     * network never holds back the results. A like toggle or comment edit that
     * lands first wins over the older load for that book.
     */
    fun setQuery(query: String) {
        val gen = ++generation
        _state.update { it.copy(query = query) }
        commentEdits.clear()
        searchJob?.cancel()
        searchJob = viewModelScope.launch {
            val hits = attempt { store.search(query) } ?: return@launch
            if (gen != generation) return@launch
            _state.update { it.copy(result = hits) }
            // One request at a time. The hosted client blocks a thread per call
            // and cancel cannot interrupt it, so a fan-out across every book
            // could fill the IO pool with calls for queries already gone.
            hits.posts.map { it.book.id }.distinct().forEach { bookId ->
                val rev = likeRevision[bookId] ?: 0L
                val likes = attempt { store.likes(bookId) }
                if (likes != null && gen == generation && rev == (likeRevision[bookId] ?: 0L)) {
                    _state.update { it.copy(likes = it.likes + (bookId to likes)) }
                }
                val loaded = attempt { store.commentIndex(bookId) } ?: CommentIndex()
                if (gen == generation) {
                    val edits = commentEdits[bookId].orEmpty()
                    val comments = edits.entries.fold(loaded) { index, (postId, thread) ->
                        index.afterThread(postId, thread)
                    }
                    _state.update { it.copy(comments = it.comments + (bookId to comments)) }
                }
            }
        }
    }

    fun onCommentsChanged(bookId: String, postId: String, comments: List<PostComment>) {
        commentEdits.getOrPut(bookId) { mutableMapOf() }[postId] = comments
        _state.update { now ->
            val current = now.comments[bookId] ?: CommentIndex()
            now.copy(comments = now.comments + (bookId to current.afterThread(postId, comments)))
        }
    }

    fun toggleLike(bookId: String, postId: String) {
        viewModelScope.launch {
            val next = store.toggleLike(bookId, postId)
            likeRevision[bookId] = (likeRevision[bookId] ?: 0L) + 1
            _state.update { it.copy(likes = it.likes + (bookId to next)) }
        }
    }

    /** Runs [block] and returns null on failure, but lets cancellation through. */
    private suspend fun <T> attempt(block: suspend () -> T): T? = try {
        block()
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        null
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
