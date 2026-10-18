package com.mewo.reader.ui.profile

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.HostedException
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.ProfilePostRecord
import com.mewo.reader.data.ProfileRecord
import com.mewo.reader.data.ProfileRepostRecord
import com.mewo.reader.data.commitRepost
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class ProfileTab { Posts, Reposts, Books }

data class ProfileUiState(
    val handle: String,
    val record: ProfileRecord? = null,
    val missing: Boolean = false,
    val tab: ProfileTab = ProfileTab.Posts,
    val posts: List<ProfilePostRecord> = emptyList(),
    val reposts: List<ProfileRepostRecord> = emptyList(),
    val books: List<BookRecord> = emptyList(),
    val postsCursor: String? = null,
    val repostsCursor: String? = null,
    val likes: Map<String, Set<String>> = emptyMap(),
    val comments: Map<String, CommentIndex> = emptyMap(),
    val loading: Boolean = true,
    val more: Boolean = false,
    val error: String? = null,
) {
    val postCount: Int? get() = record?.postCount
}

class ProfileViewModel(
    private val store: LibraryStore,
    handle: String,
) : ViewModel() {
    private val _state = MutableStateFlow(ProfileUiState(handle = handle))
    val state: StateFlow<ProfileUiState> = _state.asStateFlow()
    private var session: String? = "local"
    private var generation = 0L
    private var job: Job? = null
    private val repostTicket = mutableMapOf<String, Long>()

    fun bindSession(id: String?) {
        if (id == session) return
        session = id
        generation++
        job?.cancel()
        if (id == null) {
            _state.update { it.copy(loading = false, posts = emptyList(), reposts = emptyList(), books = emptyList()) }
        }
    }

    fun select(tab: ProfileTab) {
        if (tab == _state.value.tab) return
        _state.update { it.copy(tab = tab, error = null) }
        refresh()
    }

    fun refresh() {
        val handle = _state.value.handle
        val id = session ?: return
        val gen = ++generation
        job?.cancel()
        val now = _state.value
        val visibleEmpty = when (now.tab) {
            ProfileTab.Posts -> now.posts.isEmpty()
            ProfileTab.Reposts -> now.reposts.isEmpty()
            ProfileTab.Books -> now.books.isEmpty()
        }
        _state.update {
            it.copy(loading = visibleEmpty || it.record == null, error = null, missing = false, more = false)
        }
        job = viewModelScope.launch {
            if (!store.hostedSocial) {
                loadLocal(handle, gen, id)
                return@launch
            }
            try {
                val record = store.profile(handle)
                if (!current(gen, id)) return@launch
                _state.update { it.copy(record = record, missing = false) }
                loadTab(handle, _state.value.tab, null, gen, id)
                if (!current(gen, id)) return@launch
                _state.update { it.copy(loading = false, error = null) }
            } catch (error: Exception) {
                if (!current(gen, id)) return@launch
                if (error is HostedException && error.code == 404) {
                    _state.update { it.copy(loading = false, missing = true, record = null, error = null) }
                } else {
                    _state.update {
                        it.copy(loading = false, error = error.message ?: "Could not load this profile.")
                    }
                }
            }
        }
    }

    fun loadMore() {
        val now = _state.value
        if (now.more || now.loading) return
        val cursor = when (now.tab) {
            ProfileTab.Posts -> now.postsCursor
            ProfileTab.Reposts -> now.repostsCursor
            ProfileTab.Books -> null
        } ?: return
        val id = session ?: return
        val gen = ++generation
        _state.update { it.copy(more = true) }
        job = viewModelScope.launch {
            try {
                loadTab(now.handle, now.tab, cursor, gen, id)
            } catch (error: kotlinx.coroutines.CancellationException) {
                throw error
            } catch (error: Exception) {
                if (current(gen, id)) {
                    _state.update { it.copy(error = error.message ?: "Could not load this profile.") }
                }
            } finally {
                if (current(gen, id)) _state.update { it.copy(more = false) }
            }
        }
    }

    private suspend fun loadLocal(handle: String, gen: Long, id: String) {
        val replies = store.profileReplies(handle)
        if (!current(gen, id)) return
        val posts = replies.map { ProfilePostRecord(it.book, it.post, it.comment) }
        val bookIds = posts.map { it.book.id }.distinct()
        val likes = bookIds.associateWith { bookId -> runCatching { store.likes(bookId) }.getOrDefault(emptySet()) }
        val comments = bookIds.associateWith { bookId ->
            runCatching { store.commentIndex(bookId) }.getOrDefault(CommentIndex())
        }
        _state.update {
            it.copy(posts = posts, likes = likes, comments = comments, loading = false, error = null, missing = false)
        }
    }

    private suspend fun loadTab(handle: String, tab: ProfileTab, cursor: String?, gen: Long, id: String) {
        when (tab) {
            ProfileTab.Posts -> {
                val page = store.profilePosts(handle, cursor)
                if (!current(gen, id)) return
                _state.update {
                    it.copy(
                        posts = if (cursor == null) page.items else it.posts + page.items,
                        postsCursor = page.nextCursor,
                    )
                }
                rememberSocial(page.items.map { it.book.id })
            }
            ProfileTab.Reposts -> {
                val page = store.profileReposts(handle, cursor)
                if (!current(gen, id)) return
                _state.update {
                    it.copy(
                        reposts = if (cursor == null) page.items else it.reposts + page.items,
                        repostsCursor = page.nextCursor,
                    )
                }
                rememberSocial(page.items.map { it.book.id })
            }
            ProfileTab.Books -> {
                val page = store.profileBooks(handle)
                if (!current(gen, id)) return
                _state.update { it.copy(books = page.items) }
            }
        }
    }

    private suspend fun rememberSocial(bookIds: List<String>) {
        val ids = bookIds.distinct()
        val started = ids.associateWith { id -> repostTicket[id] ?: 0L }
        val likes = ids.associateWith { id ->
            runCatching { store.likes(id) }.getOrDefault(_state.value.likes[id] ?: emptySet())
        }
        val comments = ids.associateWith { id ->
            runCatching { store.commentIndex(id) }.getOrDefault(_state.value.comments[id] ?: CommentIndex())
        }
        _state.update { state ->
            val fresh = comments.filterKeys { id -> (repostTicket[id] ?: 0L) == started[id] }
            state.copy(likes = state.likes + likes, comments = state.comments + fresh)
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
        repostTicket[bookId] = (repostTicket[bookId] ?: 0L) + 1
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

    fun onCommentsChanged(bookId: String, postId: String, comments: List<PostComment>) {
        _state.update { now ->
            val current = now.comments[bookId] ?: CommentIndex()
            now.copy(comments = now.comments + (bookId to current.afterThread(postId, comments)))
        }
    }

    private fun current(gen: Long, id: String): Boolean = gen == generation && id == session

    companion object {
        fun factory(store: LibraryStore, handle: String) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return ProfileViewModel(store, handle) as T
            }
        }
    }
}
