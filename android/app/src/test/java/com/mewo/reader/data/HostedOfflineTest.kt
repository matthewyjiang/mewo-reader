package com.mewo.reader.data

import com.mewo.reader.ui.reader.ReaderViewModel
import java.io.File
import java.io.IOException
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
@OptIn(ExperimentalCoroutinesApi::class)
class HostedOfflineTest {
    private val json = Json { ignoreUnknownKeys = true }
    private val context = RuntimeEnvironment.getApplication()
    private var job = SupervisorJob()
    private var scope = CoroutineScope(job + Dispatchers.Default)

    @Before
    fun setUp() {
        job = SupervisorJob()
        scope = CoroutineScope(job + Dispatchers.Default)
        Dispatchers.setMain(Dispatchers.Unconfined)
        hostedPostsDecodes.set(0)
        context.filesDir.listFiles()?.forEach { it.deleteRecursively() }
        context.getSharedPreferences("mewo_hosted", 0).edit().clear().commit()
    }

    @After
    fun tearDown() {
        job.cancel()
        Dispatchers.resetMain()
    }

    @Test
    fun readerOpensCachedPostsWhenSocialReadsThrow() = runBlocking {
        val client = OfflineClient()
        client.postsError = IOException("offline")
        client.likesError = IOException("offline")
        client.indexError = IOException("offline")
        writePosts(A, "cached line")
        val store = open(client, listOf(book(A, importedAt = 3)))

        val vm = ReaderViewModel(store, A)
        withTimeout(5_000) { while (vm.state.value.loading) delay(20) }

        assertNull(vm.state.value.error)
        assertEquals("cached line", vm.state.value.posts.single().text)
        assertTrue(vm.state.value.likes.isEmpty())
        assertEquals(0, vm.state.value.comments.count("p"))
        assertTrue(client.postsCalls.isEmpty())
    }

    @Test
    fun readerUsesCachedSocialWhenTheNetworkThrows() = runBlocking {
        val client = OfflineClient()
        writePosts(A, "cached line")
        val store = open(client, listOf(book(A, importedAt = 3)))
        assertEquals(setOf("p"), store.likes(A))
        assertEquals(1, store.commentIndex(A).count("p"))
        client.likesError = IOException("offline")
        client.indexError = IOException("offline")

        val vm = ReaderViewModel(store, A)
        withTimeout(5_000) { while (vm.state.value.loading) delay(20) }

        assertNull(vm.state.value.error)
        assertEquals(setOf("p"), vm.state.value.likes)
        assertEquals(1, vm.state.value.comments.count("p"))
    }

    @Test
    fun socialCacheIsWrittenAndUsedOnlyForTransportFailure() = runBlocking {
        val client = OfflineClient()
        val store = open(client, listOf(book(A, importedAt = 3)))
        assertEquals(setOf("p"), store.likes(A))
        assertEquals("hi", store.comments(A, "p").single().text)
        val cache = readSocial(A)
        assertEquals(setOf("p"), cache.likes)
        assertEquals("hi", cache.threads.getValue("p").single().text)
        assertEquals(1, cache.index.count("p"))

        client.likes = setOf("p", "q")
        store.likes(A)
        assertEquals(setOf("p", "q"), readSocial(A).likes)
        assertEquals("hi", readSocial(A).threads.getValue("p").single().text)

        client.likesError = IOException("offline")
        assertEquals(setOf("p", "q"), store.likes(A))
        client.commentsError = IOException("offline")
        assertEquals("hi", store.comments(A, "p").single().text)
        try {
            store.comments(A, "missing")
            fail("missing thread should stay offline")
        } catch (err: IOException) {
            assertEquals("You're offline.", err.message)
        }

        client.likesError = HostedException("nope", 500)
        try {
            store.likes(A)
            fail("HTTP errors must not fall back")
        } catch (err: HostedException) {
            assertEquals(500, err.code)
        }
        assertEquals(setOf("p", "q"), readSocial(A).likes)

        writeSocial(A, """{"version":2,"likes":["z"]}""")
        client.likesError = IOException("offline")
        try {
            store.likes(A)
            fail("a different cache version is a miss")
        } catch (err: IOException) {
            assertEquals("offline", err.message)
        }
    }

    @Test
    fun downloadedIsTheCurrentPostsFileOnTheShelf() = runBlocking {
        val client = OfflineClient()
        writePosts(A, "secret")
        writePosts(B, "other", version = 2)
        val store = open(client, listOf(book(A, importedAt = 3), book(B, importedAt = 2)))
        assertTrue(store.offline.value[A] is OfflineState.Downloaded)
        assertTrue(store.offline.value[B] is OfflineState.NotDownloaded)
        assertEquals("secret", store.feed(A).single().text)
        assertTrue(client.postsCalls.isEmpty())

        assertEquals("secret", store.feed(B).single().text)
        assertEquals(listOf(B), client.postsCalls.toList())
        assertTrue(store.offline.value[B] is OfflineState.Downloaded)
        assertEquals(1, readPostsVersion(B))
    }

    @Test
    fun cachedTextIsRefusedOnceTheBookLeavesTheShelf() = runBlocking {
        val client = OfflineClient()
        client.libraryGate = CompletableDeferred()
        writePosts(B, "secret")
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(SERVER)
        val store = HostedLibraryStore(context, EpubOpener(context), sessions, client, scope)
        sessions.setSignedIn("ada", "token-a")
        withTimeout(5_000) { client.libraryEntered.await() }
        assertTrue(
            store.loadState.value is LibraryLoading || store.loadState.value.isRefreshing(),
        )
        assertEquals("secret", store.feed(B).single().text)
        assertTrue(client.postsCalls.isEmpty())

        client.libraryGate?.complete(listOf(book(A, importedAt = 3)))
        withTimeout(5_000) {
            while (store.loadState.value.isRefreshing() || store.loadState.value is LibraryLoading) delay(20)
        }
        try {
            store.feed(B)
            fail("served text for a book that is not on the shelf")
        } catch (err: IllegalStateException) {
            assertEquals("That book is not on your shelf.", err.message)
        }
        assertTrue(File(bookDir(B), "posts.json").exists())
        assertTrue(client.postsCalls.isEmpty())
        assertEquals(listOf(A), store.library.value.books.map { it.id })
    }

    @Test
    fun setDownloadedWritesPostsAndSocialAndRemoveKeepsTheCover() = runBlocking {
        val client = OfflineClient()
        val store = open(client, listOf(book(A, importedAt = 3)))
        val cover = File(bookDir(A), "cover.jpg")
        cover.parentFile?.mkdirs()
        cover.writeBytes(byteArrayOf(1, 2, 3))

        store.setDownloaded(A, true)
        assertTrue(store.offline.value[A] is OfflineState.Downloaded)
        assertTrue(File(bookDir(A), "posts.json").exists())
        assertEquals(setOf("p"), readSocial(A).likes)
        assertEquals(1, client.postsCalls.size)

        store.setDownloaded(A, true)
        assertEquals(1, client.postsCalls.size)

        store.setDownloaded(A, false)
        assertTrue(store.offline.value[A] is OfflineState.NotDownloaded)
        assertFalse(File(bookDir(A), "posts.json").exists())
        assertFalse(File(bookDir(A), "social.json").exists())
        assertEquals(3, cover.length())
    }

    @Test
    fun aFailedDownloadLeavesNoPostsFile() = runBlocking {
        val client = OfflineClient()
        client.likesError = IOException("offline")
        val store = open(client, listOf(book(A, importedAt = 3)))
        store.setDownloaded(A, true)
        val failed = store.offline.value[A]
        assertTrue(failed is OfflineState.Failed)
        assertEquals("offline", (failed as OfflineState.Failed).message)
        assertFalse(File(bookDir(A), "posts.json").exists())
    }

    @Test
    fun downloadProgressIsVisibleAndATapCancels() = runBlocking {
        val client = OfflineClient()
        client.blockPosts = true
        client.progress = 0.4f
        val store = open(client, listOf(book(A, importedAt = 3)))
        val running = scope.launch { store.setDownloaded(A, true) }
        withTimeout(5_000) {
            while (store.offline.value[A] !is OfflineState.Downloading) delay(10)
        }
        val mark = store.offline.value[A] as OfflineState.Downloading
        assertEquals(0.4f, mark.fraction!!, 0.001f)

        store.setDownloaded(A, false)
        running.join()
        assertTrue(store.offline.value[A] is OfflineState.NotDownloaded)
        assertFalse(File(bookDir(A), "posts.json").exists())
    }

    @Test
    fun downloadAllRunsTwoAtATimeInShelfOrder() = runBlocking {
        val client = OfflineClient()
        client.blockPosts = true
        val books = listOf(
            book(A, importedAt = 40),
            book(B, importedAt = 30),
            book(C, importedAt = 20),
            book(D, importedAt = 10),
        )
        val store = open(client, books)
        val order = store.library.value.books.map { it.id }
        assertEquals(listOf(A, B, C, D), order)
        val running = scope.launch { store.downloadAll() }
        withTimeout(5_000) {
            while (client.postsCalls.size < 2) delay(10)
        }
        delay(80)
        assertEquals(order.take(2).toSet(), client.postsCalls.toSet())
        assertTrue(client.maxInFlight.get() <= 2)
        assertTrue(store.offlineSummary.value.running)
        assertEquals(0, store.offlineSummary.value.downloaded)
        assertEquals(4, store.offlineSummary.value.total)

        client.hold.countDown()
        withTimeout(5_000) { running.join() }
        assertEquals(4, client.postsCalls.size)
        assertEquals(order.toSet(), client.postsCalls.toSet())
        assertTrue(client.maxInFlight.get() <= 2)
        assertEquals(OfflineSummary(4, 4, running = false), store.offlineSummary.value)
        assertTrue(order.all { File(bookDir(it), "posts.json").exists() })
    }

    @Test
    fun downloadAllCancelsWhenTappedAgain() = runBlocking {
        val client = OfflineClient()
        client.blockPosts = true
        val books = listOf(
            book(A, importedAt = 40),
            book(B, importedAt = 30),
            book(C, importedAt = 20),
            book(D, importedAt = 10),
        )
        val store = open(client, books)
        val first = scope.launch { store.downloadAll() }
        withTimeout(5_000) {
            while (client.entered.get() < 2) delay(10)
        }
        assertTrue(store.offlineSummary.value.running)
        val second = scope.launch { store.downloadAll() }
        withTimeout(8_000) {
            first.join()
            second.join()
        }
        assertEquals(2, client.postsCalls.size)
        assertFalse(store.offlineSummary.value.running)
        assertEquals(0, store.offlineSummary.value.downloaded)
        books.forEach { assertFalse(File(bookDir(it.id), "posts.json").exists()) }
    }

    @Test
    fun switchingAccountsDropsSocialCacheAndKeepsPosts() = runBlocking {
        val client = OfflineClient()
        writePosts(A, "secret")
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(SERVER)
        val store = HostedLibraryStore(context, EpubOpener(context), sessions, client, scope)
        client.shelf.add(book(A, importedAt = 3))
        sessions.setSignedIn("ada", "token-a")
        withTimeout(5_000) {
            while (store.loadState.value !is LibraryLoaded || store.loadState.value.isRefreshing()) delay(20)
        }
        store.likes(A)
        assertTrue(File(bookDir(A), "social.json").exists())

        client.shelf.clear()
        client.shelf.add(book(B, importedAt = 1))
        sessions.setSignedIn("bea", "token-b")
        withTimeout(5_000) {
            while (store.library.value.books.none { it.id == B }) delay(20)
        }
        assertFalse(File(bookDir(A), "social.json").exists())
        assertTrue(File(bookDir(A), "posts.json").exists())
        try {
            store.feed(A)
            fail("Ada's download must not open on Bea's shelf")
        } catch (err: IllegalStateException) {
            assertEquals("That book is not on your shelf.", err.message)
        }
    }

    @Test
    fun aCachedShelfCountsDownloadsWhenRefreshFails() = runBlocking {
        val client = OfflineClient()
        client.libraryError = IOException("offline")
        writePosts(A, "secret")
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(SERVER)
        ShelfCache(File(context.filesDir, "hosted-shelves"), json)
            .write(SERVER, "ada", listOf(book(A, importedAt = 3)))
        val store = HostedLibraryStore(context, EpubOpener(context), sessions, client, scope)
        sessions.setSignedIn("ada", "token-a")
        withTimeout(5_000) {
            while (store.loadState.value !is LibraryFailed) delay(20)
        }

        assertTrue(store.offline.value[A] is OfflineState.Downloaded)
        assertEquals(OfflineSummary(1, 1, running = false), store.offlineSummary.value)
    }

    @Test
    fun savingProgressDoesNotRereadPosts() = runBlocking {
        val client = OfflineClient()
        writePosts(A, "secret")
        val store = open(client, listOf(book(A, importedAt = 3)))
        val first = hostedPostsDecodes.get()
        assertTrue(first > 0)

        repeat(6) { index -> store.saveProgress(A, index) }

        assertEquals(first, hostedPostsDecodes.get())
    }

    @Test
    fun leavingTheCallerDoesNotCancelTheDownload() = runBlocking {
        val client = OfflineClient()
        client.blockPosts = true
        val store = open(client, listOf(book(A, importedAt = 3)))
        val caller = CoroutineScope(SupervisorJob())
        val running = caller.launch { store.setDownloaded(A, true) }
        withTimeout(5_000) {
            while (client.entered.get() < 1) delay(10)
        }

        caller.cancel()
        running.join()
        client.hold.countDown()
        withTimeout(5_000) {
            while (store.offline.value[A] !is OfflineState.Downloaded) delay(10)
        }
        assertTrue(File(bookDir(A), "posts.json").exists())
        caller.cancel()
    }

    @Test
    fun aFailedDownloadClearsOnceThePostsExist() = runBlocking {
        val client = OfflineClient()
        client.likesError = IOException("offline")
        val store = open(client, listOf(book(A, importedAt = 3)))
        store.setDownloaded(A, true)
        assertTrue(store.offline.value[A] is OfflineState.Failed)

        client.likesError = null
        assertEquals("secret", store.feed(A).single().text)
        assertTrue(store.offline.value[A] is OfflineState.Downloaded)

        client.likesError = IOException("offline")
        store.setDownloaded(A, false)
        store.setDownloaded(A, true)
        assertTrue(store.offline.value[A] is OfflineState.Failed)
        writePosts(A, "secret")
        client.likesError = null
        store.setDownloaded(A, true)
        assertTrue(store.offline.value[A] is OfflineState.Downloaded)
        assertEquals(3, client.postsCalls.size)
    }

    @Test
    fun aBookThatLeavesTheShelfDropsSocialCache() = runBlocking {
        val client = OfflineClient()
        writePosts(B, "secret")
        writeSocial(B, json.encodeToString(SocialCache(likes = setOf("p"))))
        val store = open(client, listOf(book(A, importedAt = 3), book(B, importedAt = 2)))
        client.shelf.clear()
        client.shelf.add(book(A, importedAt = 3))
        store.load()
        withTimeout(5_000) {
            while (store.library.value.books.any { it.id == B } || store.loadState.value.isRefreshing()) {
                delay(20)
            }
        }

        assertFalse(File(bookDir(B), "posts.json").exists())
        assertFalse(File(bookDir(B), "social.json").exists())
    }

    @Test
    fun signOutDuringADownloadLeavesTheCacheEmpty() = runBlocking {
        val client = OfflineClient()
        client.blockPosts = true
        client.blockIndex = true
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(SERVER)
        val store = HostedLibraryStore(context, EpubOpener(context), sessions, client, scope)
        client.shelf.add(book(A, importedAt = 3))
        sessions.setSignedIn("ada", "token-a")
        withTimeout(5_000) {
            while (store.loadState.value !is LibraryLoaded || store.loadState.value.isRefreshing()) delay(20)
        }
        val hosted = File(context.filesDir, "hosted")
        val marker = File(hosted, "marker")
        marker.writeText("x")
        val downloading = scope.launch { store.setDownloaded(A, true) }
        withTimeout(5_000) {
            while (client.entered.get() < 1) delay(10)
        }
        val reading = scope.launch { store.commentIndex(A) }
        withTimeout(5_000) {
            while (client.indexEntered.get() < 1) delay(10)
        }

        sessions.signOut()
        withTimeout(5_000) {
            while (marker.exists()) delay(10)
        }
        client.indexHold.countDown()
        withTimeout(5_000) { reading.join() }
        downloading.join()
        val files = hosted.walkTopDown().filter { it.isFile }.map { it.name }.toList()
        assertEquals(emptyList<String>(), files)
    }

    @Test
    fun twoDownloadsFinishDownloaded() = runBlocking {
        val client = OfflineClient()
        val store = open(client, listOf(book(A, importedAt = 3), book(B, importedAt = 2)))
        repeat(40) {
            client.blockPosts = true
            client.hold = CountDownLatch(1)
            client.entered.set(0)
            val first = scope.launch { store.setDownloaded(A, true) }
            val second = scope.launch { store.setDownloaded(B, true) }
            withTimeout(5_000) {
                while (client.entered.get() < 2) delay(5)
            }
            client.hold.countDown()
            withTimeout(5_000) {
                first.join()
                second.join()
            }
            assertTrue(store.offline.value[A] is OfflineState.Downloaded)
            assertTrue(store.offline.value[B] is OfflineState.Downloaded)
            store.setDownloaded(A, false)
            store.setDownloaded(B, false)
        }
    }

    private fun open(client: OfflineClient, books: List<BookRecord>): HostedLibraryStore {
        client.shelf.clear()
        client.shelf.addAll(books)
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(SERVER)
        val store = HostedLibraryStore(context, EpubOpener(context), sessions, client, scope)
        runBlocking {
            sessions.setSignedIn("ada", "token-a")
            val want = books.map { it.id }.toSet()
            withTimeout(5_000) {
                while (
                    store.library.value.books.map { it.id }.toSet() != want ||
                    store.loadState.value !is LibraryLoaded ||
                    store.loadState.value.isRefreshing()
                ) {
                    delay(20)
                }
            }
        }
        return store
    }

    private fun writePosts(id: String, text: String, version: Int = 1) {
        val dir = bookDir(id).apply { mkdirs() }
        File(dir, "posts.json").writeText(
            json.encodeToString(
                FeedCache(
                    version = version,
                    posts = listOf(FeedPost(id = "p", kind = PostKind.Body, text = text, chapter = "1")),
                ),
            ),
        )
    }

    private fun writeSocial(id: String, text: String) {
        val dir = bookDir(id).apply { mkdirs() }
        File(dir, "social.json").writeText(text)
    }

    private fun readSocial(id: String): SocialCache =
        json.decodeFromString(File(bookDir(id), "social.json").readText())

    private fun readPostsVersion(id: String): Int =
        json.decodeFromString<FeedCache>(File(bookDir(id), "posts.json").readText()).version

    private fun bookDir(id: String) = File(File(context.filesDir, "hosted"), id)

    private fun book(id: String, importedAt: Long) = BookRecord(
        id = id,
        title = id,
        author = "Austen",
        handle = "austen",
        importedAt = importedAt,
        postCount = 1,
        mine = true,
        shared = false,
        ownerUsername = "ada",
    )

    private class OfflineClient : HostedClient() {
        val shelf = mutableListOf<BookRecord>()
        var libraryGate: CompletableDeferred<List<BookRecord>>? = null
        val libraryEntered = CompletableDeferred<Unit>()
        var libraryError: Exception? = null
        var postsError: Exception? = null
        var likesError: Exception? = null
        var indexError: Exception? = null
        var commentsError: Exception? = null
        var likes = setOf("p")
        var index = CommentIndex(counts = mapOf("p" to 1))
        var thread = listOf(
            PostComment(id = "c", username = "ada", text = "hi", createdAt = 1, mine = true),
        )
        val postsCalls = ConcurrentLinkedQueue<String>()
        val entered = AtomicInteger(0)
        val maxInFlight = AtomicInteger(0)
        var hold = CountDownLatch(1)
        var blockPosts = false
        var blockIndex = false
        val indexEntered = AtomicInteger(0)
        val indexHold = CountDownLatch(1)
        var progress: Float? = 0.4f

        override fun library(session: HostedSession): LibrarySnapshot {
            libraryError?.let { throw it }
            val gate = libraryGate
            if (gate != null) {
                libraryEntered.complete(Unit)
                return runBlocking { LibrarySnapshot(gate.await()) }
            }
            return LibrarySnapshot(shelf.toList())
        }

        override fun posts(
            session: HostedSession,
            id: String,
            onProgress: (Float?) -> Unit,
        ): List<FeedPost> {
            postsCalls.add(id)
            val now = entered.incrementAndGet()
            maxInFlight.updateAndGet { current -> maxOf(current, now) }
            onProgress(progress)
            if (blockPosts) {
                try {
                    if (!hold.await(5, TimeUnit.SECONDS)) throw IOException("timed out")
                } catch (err: InterruptedException) {
                    throw err
                }
            }
            entered.decrementAndGet()
            postsError?.let { throw it }
            return listOf(FeedPost(id = "p", kind = PostKind.Body, text = "secret", chapter = "1"))
        }

        override fun downloadCover(session: HostedSession, id: String, dest: File): Boolean = false

        override fun likes(session: HostedSession, id: String): Set<String> {
            likesError?.let { throw it }
            return likes
        }

        override fun commentIndex(session: HostedSession, id: String): CommentIndex {
            if (blockIndex) {
                indexEntered.incrementAndGet()
                try {
                    if (!indexHold.await(5, TimeUnit.SECONDS)) throw IOException("timed out")
                } catch (err: InterruptedException) {
                    throw err
                }
            }
            indexError?.let { throw it }
            return index
        }

        override fun patch(session: HostedSession, id: String, body: PatchBook): BookRecord {
            val current = shelf.first { it.id == id }
            return current.copy(progressIndex = body.progressIndex ?: current.progressIndex)
        }

        override fun comments(session: HostedSession, id: String, postId: String): List<PostComment> {
            commentsError?.let { throw it }
            return thread
        }
    }

    companion object {
        private const val SERVER = "https://mewo.example"
        private const val A = "11111111-1111-1111-1111-111111111111"
        private const val B = "22222222-2222-2222-2222-222222222222"
        private const val C = "33333333-3333-3333-3333-333333333333"
        private const val D = "44444444-4444-4444-4444-444444444444"
    }
}
