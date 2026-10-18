package com.mewo.reader.ui.library

import android.net.Uri
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.LibraryEvent
import com.mewo.reader.data.LibraryFailed
import com.mewo.reader.data.LibraryLoad
import com.mewo.reader.data.LibraryLoaded
import com.mewo.reader.data.LibrarySnapshot
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.books
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.PostHit
import com.mewo.reader.data.ProfileReply
import com.mewo.reader.data.reduceLibraryLoad
import java.io.File
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class LibraryViewModelTest {
    private lateinit var main: TestDispatcher

    @Before
    fun setUp() {
        main = kotlinx.coroutines.test.StandardTestDispatcher()
        Dispatchers.setMain(main)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun refreshKeepsCachedBooksAndCallsTheServerAgainAfterItFinishes() = runTest(main) {
        val store = FakeStore(listOf(sample("cached")))
        val vm = LibraryViewModel(store)
        runCurrent()
        val loading = vm.state.value.load
        assertTrue(loading is LibraryLoaded)
        assertTrue((loading as LibraryLoaded).refreshing)
        assertEquals("cached", loading.books.single().id)
        assertEquals(1, store.loads)

        vm.refresh()
        runCurrent()
        assertEquals(1, store.loads)

        store.release()
        advanceUntilIdle()
        val done = vm.state.value.load
        assertTrue(done is LibraryLoaded)
        assertFalse((done as LibraryLoaded).refreshing)
        assertEquals("fresh", done.books.single().id)

        store.holdNext = false
        vm.refresh()
        advanceUntilIdle()
        assertEquals(2, store.loads)
        assertFalse((vm.state.value.load as LibraryLoaded).refreshing)
    }

    @Test
    fun failedRefreshKeepsTheCachedShelf() = runTest(main) {
        val store = FakeStore(listOf(sample("cached")))
        store.fail = true
        val vm = LibraryViewModel(store)
        store.release()
        advanceUntilIdle()
        val load = vm.state.value.load
        assertTrue(load is LibraryFailed)
        assertEquals("cached", load.books().single().id)
        assertEquals("offline", (load as LibraryFailed).message)
    }

    private class FakeStore(cached: List<BookRecord>) : LibraryStore {
        private val _load = MutableStateFlow<LibraryLoad>(LibraryLoaded(cached, refreshing = false))
        override val loadState: StateFlow<LibraryLoad> = _load.asStateFlow()
        private val _library = MutableStateFlow(LibrarySnapshot(cached))
        override val library: StateFlow<LibrarySnapshot> = _library.asStateFlow()
        var loads = 0
        var fail = false
        var holdNext = true
        private var gate = CompletableDeferred<Unit>()

        fun release() {
            gate.complete(Unit)
        }

        override suspend fun load() {
            loads += 1
            _load.value = reduceLibraryLoad(_load.value, LibraryEvent.RefreshStarted)
            if (holdNext) gate.await()
            if (fail) {
                _load.value = reduceLibraryLoad(_load.value, LibraryEvent.RefreshFailed("offline"))
                error("offline")
            }
            val books = listOf(sample("fresh"))
            _library.value = LibrarySnapshot(books)
            _load.value = reduceLibraryLoad(_load.value, LibraryEvent.RefreshSucceeded(books))
        }

        override suspend fun importFromUri(uri: Uri): BookRecord = error("unused")
        override suspend fun importSample(): BookRecord = error("unused")
        override suspend fun delete(id: String) = Unit
        override suspend fun setShared(id: String, shared: Boolean) = Unit
        override suspend fun book(id: String): BookRecord? = null
        override suspend fun feed(id: String): List<FeedPost> = emptyList()
        override suspend fun saveProgress(id: String, index: Int) = Unit
        override suspend fun likes(id: String): Set<String> = emptySet()
        override suspend fun toggleLike(id: String, postId: String): Set<String> = emptySet()
        override suspend fun commentIndex(id: String): CommentIndex = CommentIndex()
        override suspend fun comments(id: String, postId: String): List<PostComment> = emptyList()
        override suspend fun addComment(id: String, postId: String, text: String): List<PostComment> =
            emptyList()
        override suspend fun deleteComment(
            id: String,
            postId: String,
            commentId: String,
        ): List<PostComment> = emptyList()
        override suspend fun search(query: String) = com.mewo.reader.data.SearchResult()
        override suspend fun likedPosts(): List<PostHit> = emptyList()
        override suspend fun profileReplies(handle: String): List<ProfileReply> = emptyList()
        override fun coverFile(id: String): File? = null
    }

    private companion object {
        fun sample(id: String) = BookRecord(
            id = id,
            title = id,
            author = "Austen",
            handle = "austen",
            importedAt = 1,
            postCount = 4,
            progressIndex = 1,
            mine = true,
            shared = false,
            ownerUsername = "ada",
        )
    }
}
