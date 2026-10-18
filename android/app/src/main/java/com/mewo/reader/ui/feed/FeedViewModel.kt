package com.mewo.reader.ui.feed

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPhase
import com.mewo.reader.data.HomeFeedCache
import com.mewo.reader.data.HomeFeedPage
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.MemoryFeedCache
import com.mewo.reader.data.beginFeedRefresh
import com.mewo.reader.data.commitRepost
import com.mewo.reader.data.failFeed
import com.mewo.reader.data.finishFeedRefresh
import com.mewo.reader.data.phaseFromFeedCache
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class FeedUiState(
    val phase: FeedPhase = FeedPhase.Loading,
    val session: String? = null,
    val likes: Map<String, Set<String>> = emptyMap(),
    val comments: Map<String, CommentIndex> = emptyMap(),
)

class FeedViewModel(
    private val store: LibraryStore,
    private val cache: HomeFeedCache = MemoryFeedCache(),
) : ViewModel() {
    private val _state = MutableStateFlow(FeedUiState())
    val state: StateFlow<FeedUiState> = _state.asStateFlow()
    private var session: String? = null
    private var generation = 0L
    private var job: Job? = null

    fun bindSession(id: String?) {
        if (id == session) return
        session = id
        generation++
        job?.cancel()
        if (id == null) {
            _state.update { it.copy(session = null, phase = FeedPhase.Loading) }
            return
        }
        val cached = if (store.hostedSocial) cache.read(id) else null
        _state.update { it.copy(session = id, phase = phaseFromFeedCache(cached)) }
        refresh()
    }

    fun refresh() {
        val id = session ?: return
        val gen = ++generation
        job?.cancel()
        if (!store.hostedSocial) {
            _state.update {
                it.copy(phase = finishFeedRefresh(HomeFeedPage()))
            }
            return
        }
        _state.update { it.copy(phase = beginFeedRefresh(it.phase)) }
        job = viewModelScope.launch {
            try {
                val page = store.homeFeed(null)
                if (!current(gen, id)) return@launch
                cache.write(id, page)
                _state.update { it.copy(phase = finishFeedRefresh(page)) }
                loadSocial(page)
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                if (!current(gen, id)) return@launch
                _state.update { it.copy(phase = failFeed(it.phase, error.message ?: "Could not load the feed.")) }
            }
        }
    }

    fun loadMore() {
        val phase = _state.value.phase as? FeedPhase.Loaded ?: return
        if (phase.more || phase.page.nextCursor == null) return
        val id = session ?: return
        if (!store.hostedSocial) return
        val cursor = phase.page.nextCursor
        val gen = ++generation
        job?.cancel()
        _state.update { it.copy(phase = phase.copy(more = true)) }
        job = viewModelScope.launch {
            try {
                val page = store.homeFeed(cursor)
                if (!current(gen, id)) return@launch
                _state.update { state ->
                    val shown = (state.phase as? FeedPhase.Loaded)?.page
                    val items = (shown?.items ?: emptyList()) + page.items
                    state.copy(phase = finishFeedRefresh(HomeFeedPage(items, page.nextCursor)))
                }
                loadSocial(page)
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                if (!current(gen, id)) return@launch
                _state.update { it.copy(phase = failFeed(it.phase, error.message ?: "Could not load the feed.")) }
            }
        }
    }

    fun onCommentsChanged(bookId: String, postId: String, comments: List<PostComment>) {
        _state.update { now ->
            val current = now.comments[bookId] ?: CommentIndex()
            now.copy(comments = now.comments + (bookId to current.afterThread(postId, comments)))
        }
    }

    fun toggleLike(bookId: String, postId: String) {
        viewModelScope.launch {
            runCatching { store.toggleLike(bookId, postId) }
                .onSuccess { ids -> _state.update { it.copy(likes = it.likes + (bookId to ids)) } }
        }
    }

    fun toggleRepost(bookId: String, postId: String) {
        val before = _state.value.comments[bookId] ?: CommentIndex()
        viewModelScope.launch {
            runCatching {
                commitRepost(
                    before,
                    postId,
                    { on -> store.setRepost(bookId, postId, on) },
                    { _state.value.comments[bookId] ?: CommentIndex() },
                    { next -> _state.update { it.copy(comments = it.comments + (bookId to next)) } },
                )
            }
        }
    }

    private suspend fun loadSocial(page: HomeFeedPage) {
        val ids = page.items.map { it.book.id }.distinct()
        val likes = ids.associateWith { id ->
            runCatching { store.likes(id) }.getOrDefault(_state.value.likes[id] ?: emptySet())
        }
        val comments = ids.associateWith { id ->
            runCatching { store.commentIndex(id) }.getOrDefault(_state.value.comments[id] ?: CommentIndex())
        }
        _state.update { it.copy(likes = it.likes + likes, comments = it.comments + comments) }
    }

    private fun current(gen: Long, id: String): Boolean = gen == generation && id == session

    companion object {
        fun factory(store: LibraryStore, cache: HomeFeedCache = MemoryFeedCache()) =
            object : ViewModelProvider.Factory {
                @Suppress("UNCHECKED_CAST")
                override fun <T : ViewModel> create(modelClass: Class<T>): T {
                    return FeedViewModel(store, cache) as T
                }
            }
    }
}
