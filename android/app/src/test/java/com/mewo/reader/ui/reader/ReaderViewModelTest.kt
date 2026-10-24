package com.mewo.reader.ui.reader

import android.net.Uri
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.LibraryLoad
import com.mewo.reader.data.LibraryLoaded
import com.mewo.reader.data.LibrarySnapshot
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.PostHit
import com.mewo.reader.data.PostKind
import com.mewo.reader.data.ProfileReply
import com.mewo.reader.data.SearchResult
import java.io.File
import java.io.IOException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ReaderViewModelTest {
    private val main = StandardTestDispatcher()

    @Before
    fun setUp() {
        Dispatchers.setMain(main)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun cachedBookOpensWhenSocialReadsFail() = runTest(main) {
        val store = FakeReaderStore()
        store.likesError = IOException("offline")
        store.indexError = IOException("offline")
        val vm = ReaderViewModel(store, BOOK)

        advanceUntilIdle()

        assertNull(vm.state.value.error)
        assertFalse(vm.state.value.loading)
        assertEquals(listOf("p1"), vm.state.value.posts.map { it.id })
        assertEquals("Pride", vm.state.value.book?.title)
        assertTrue(vm.state.value.likes.isEmpty())
        assertEquals(0, vm.state.value.comments.count("p1"))
    }

    @Test
    fun aMissingBookStillFailsToOpen() = runTest(main) {
        val store = FakeReaderStore()
        store.feedError = IOException("no posts")
        val vm = ReaderViewModel(store, BOOK)

        advanceUntilIdle()

        assertEquals("no posts", vm.state.value.error)
        assertTrue(vm.state.value.posts.isEmpty())
        assertFalse(vm.state.value.loading)
    }

    @Test
    fun offlineLikeDoesNotCrashAndReverts() = runTest(main) {
        val store = FakeReaderStore()
        val vm = ReaderViewModel(store, BOOK)
        advanceUntilIdle()
        assertEquals(setOf("p1"), vm.state.value.likes)

        store.likeGate = CompletableDeferred()
        store.likeError = IOException("offline")
        vm.toggleLike("p1")
        runCurrent()
        assertFalse(vm.state.value.likes.contains("p1"))

        store.likeGate?.complete(Unit)
        advanceUntilIdle()

        assertEquals(setOf("p1"), vm.state.value.likes)
        assertNull(vm.state.value.error)
        assertEquals(listOf("p1"), vm.state.value.posts.map { it.id })
    }

    @Test
    fun twoFailedLikesRevertOnlyTheirOwnPost() = runTest(main) {
        val store = FakeReaderStore()
        val vm = ReaderViewModel(store, BOOK)
        advanceUntilIdle()

        store.likeGate = CompletableDeferred()
        store.likeError = IOException("offline")
        vm.toggleLike("p1")
        runCurrent()
        vm.toggleLike("p2")
        runCurrent()
        assertEquals(setOf("p2"), vm.state.value.likes)

        store.likeGate?.complete(Unit)
        advanceUntilIdle()

        assertEquals(setOf("p1"), vm.state.value.likes)
        assertNull(vm.state.value.error)
    }

    @Test
    fun offlineProgressDoesNotCrashTheReader() = runTest(main) {
        val store = FakeReaderStore()
        store.progressError = IOException("offline")
        val vm = ReaderViewModel(store, BOOK)
        advanceUntilIdle()

        vm.saveProgress(2)
        advanceUntilIdle()

        assertNull(vm.state.value.error)
        assertEquals(listOf("p1"), vm.state.value.posts.map { it.id })
        assertFalse(vm.state.value.loading)
    }

    private class FakeReaderStore : LibraryStore {
        var likesError: Exception? = null
        var indexError: Exception? = null
        var feedError: Exception? = null
        var likeError: Exception? = null
        var progressError: Exception? = null
        var likeGate: CompletableDeferred<Unit>? = null
        private val book = BookRecord(
            id = BOOK,
            title = "Pride",
            author = "Austen",
            handle = "austen",
            importedAt = 1,
            postCount = 1,
            progressIndex = 0,
        )
        private val posts = listOf(FeedPost("p1", PostKind.Body, "A line.", "One"))

        override val library = MutableStateFlow(LibrarySnapshot(listOf(book)))
        override val loadState: StateFlow<LibraryLoad> =
            MutableStateFlow(LibraryLoaded(listOf(book), refreshing = false))

        override suspend fun load() = Unit
        override suspend fun importFromUri(uri: Uri): BookRecord = error("unused")
        override suspend fun importSample(): BookRecord = error("unused")
        override suspend fun delete(id: String) = Unit
        override suspend fun setShared(id: String, shared: Boolean) = Unit
        override suspend fun book(id: String): BookRecord? = book.takeIf { it.id == id }
        override suspend fun feed(id: String): List<FeedPost> {
            feedError?.let { throw it }
            return posts
        }
        override suspend fun saveProgress(id: String, index: Int) {
            progressError?.let { throw it }
        }
        override suspend fun likes(id: String): Set<String> {
            likesError?.let { throw it }
            return setOf("p1")
        }
        override suspend fun toggleLike(id: String, postId: String): Set<String> {
            likeGate?.await()
            likeError?.let { throw it }
            return setOf(postId)
        }
        override suspend fun commentIndex(id: String): CommentIndex {
            indexError?.let { throw it }
            return CommentIndex(counts = mapOf("p1" to 2))
        }
        override suspend fun comments(id: String, postId: String): List<PostComment> = emptyList()
        override suspend fun addComment(id: String, postId: String, text: String): List<PostComment> =
            emptyList()
        override suspend fun deleteComment(
            id: String,
            postId: String,
            commentId: String,
        ): List<PostComment> = emptyList()
        override suspend fun search(query: String): SearchResult = SearchResult()
        override suspend fun likedPosts(): List<PostHit> = emptyList()
        override suspend fun profileReplies(handle: String): List<ProfileReply> = emptyList()
        override fun coverFile(id: String): File? = null
    }

    private companion object {
        const val BOOK = "11111111-1111-1111-1111-111111111111"
    }
}
