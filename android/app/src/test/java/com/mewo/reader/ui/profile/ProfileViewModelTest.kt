package com.mewo.reader.ui.profile

import android.net.Uri
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.HostedException
import com.mewo.reader.data.LibraryLoad
import com.mewo.reader.data.LibraryLoading
import com.mewo.reader.data.LibrarySnapshot
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.PostHit
import com.mewo.reader.data.PostKind
import com.mewo.reader.data.ProfileBooks
import com.mewo.reader.data.ProfilePostRecord
import com.mewo.reader.data.ProfilePostsPage
import com.mewo.reader.data.ProfileRecord
import com.mewo.reader.data.ProfileReply
import com.mewo.reader.data.ProfileRepostRecord
import com.mewo.reader.data.ProfileRepostsPage
import com.mewo.reader.data.RepostState
import com.mewo.reader.data.SearchResult
import com.mewo.reader.data.hasReposted
import com.mewo.reader.data.joinedLabel
import java.io.File
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
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ProfileViewModelTest {
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
    fun joinedLabelUsesUtcMonthAndYear() {
        assertEquals("Joined Jan 2021", joinedLabel(1609459200000))
    }

    @Test
    fun anUnknownAccountIsMissing() = runTest(main) {
        val store = FakeStore(missing = true)
        val vm = ProfileViewModel(store, "nobody")
        vm.refresh()
        advanceUntilIdle()
        assertTrue(vm.state.value.missing)
        assertEquals(null, vm.state.value.record)
        assertFalse(vm.state.value.loading)
    }

    @Test
    fun tabsLoadPostsRepostsAndSharedBooks() = runTest(main) {
        val store = FakeStore()
        val vm = ProfileViewModel(store, "ada")
        vm.refresh()
        advanceUntilIdle()
        assertEquals(1, vm.state.value.posts.size)
        assertEquals(2, vm.state.value.postCount)

        vm.select(ProfileTab.Reposts)
        advanceUntilIdle()
        assertEquals(1, vm.state.value.reposts.size)
        assertEquals(ProfileTab.Reposts, vm.state.value.tab)

        vm.select(ProfileTab.Books)
        advanceUntilIdle()
        assertEquals(listOf("b1"), vm.state.value.books.map { it.id })
    }

    @Test
    fun repostRollsBackWhenTheServerRejects() = runTest(main) {
        val store = FakeStore()
        val vm = ProfileViewModel(store, "ada")
        vm.refresh()
        advanceUntilIdle()
        vm.toggleRepost("b1", "0")
        runCurrent()
        assertTrue(vm.state.value.comments.getValue("b1").hasReposted("0"))
        store.reposts.completeExceptionally(RuntimeException("offline"))
        advanceUntilIdle()
        assertFalse(vm.state.value.comments.getValue("b1").hasReposted("0"))
    }

    private class FakeStore(private val missing: Boolean = false) : LibraryStore {
        override val hostedSocial: Boolean = true
        val reposts = CompletableDeferred<RepostState>()
        private val book = BookRecord(
            id = "b1", title = "Emma", author = "Austen", handle = "austen", importedAt = 1, shared = true,
        )
        private val post = FeedPost("0", PostKind.Body, "A line.", "One")

        override suspend fun profile(username: String): ProfileRecord {
            if (missing) throw HostedException("Not found.", 404)
            return ProfileRecord(username, joinedAt = 1609459200000, postCount = 2, repostCount = 1, mine = true)
        }

        override suspend fun profilePosts(username: String, cursor: String?): ProfilePostsPage {
            val comment = PostComment("c1", "ada", "Nice", createdAt = 5, mine = true)
            return ProfilePostsPage(listOf(ProfilePostRecord(book, post, comment)))
        }

        override suspend fun profileReposts(username: String, cursor: String?): ProfileRepostsPage =
            ProfileRepostsPage(listOf(ProfileRepostRecord(book, post, repostedAt = 6)))

        override suspend fun profileBooks(username: String): ProfileBooks = ProfileBooks(listOf(book))

        override suspend fun likes(id: String): Set<String> = emptySet()
        override suspend fun commentIndex(id: String): CommentIndex = CommentIndex()
        override suspend fun setRepost(id: String, postId: String, reposted: Boolean): RepostState =
            reposts.await()

        override val library: StateFlow<LibrarySnapshot> = MutableStateFlow(LibrarySnapshot())
        override val loadState: StateFlow<LibraryLoad> = MutableStateFlow(LibraryLoading)
        override suspend fun load() = Unit
        override suspend fun importFromUri(uri: Uri): BookRecord = error("unused")
        override suspend fun importSample(): BookRecord = error("unused")
        override suspend fun delete(id: String) = Unit
        override suspend fun setShared(id: String, shared: Boolean) = Unit
        override suspend fun book(id: String): BookRecord? = null
        override suspend fun feed(id: String): List<FeedPost> = emptyList()
        override suspend fun saveProgress(id: String, index: Int) = Unit
        override suspend fun search(query: String): SearchResult = SearchResult()
        override suspend fun comments(id: String, postId: String): List<PostComment> = emptyList()
        override suspend fun addComment(id: String, postId: String, text: String): List<PostComment> =
            emptyList()
        override suspend fun deleteComment(id: String, postId: String, commentId: String): List<PostComment> =
            emptyList()
        override suspend fun toggleLike(id: String, postId: String): Set<String> = emptySet()
        override suspend fun likedPosts(): List<PostHit> = emptyList()
        override suspend fun profileReplies(handle: String): List<ProfileReply> = emptyList()
        override fun coverFile(id: String): File? = null
    }
}
