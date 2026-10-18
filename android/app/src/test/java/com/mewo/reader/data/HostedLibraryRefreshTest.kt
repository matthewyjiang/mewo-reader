package com.mewo.reader.data

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.io.File
import java.util.concurrent.ConcurrentHashMap

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class HostedLibraryRefreshTest {
    private val json = Json { ignoreUnknownKeys = true }
    private val context = RuntimeEnvironment.getApplication()
    private var job = SupervisorJob()
    private var scope = CoroutineScope(job + Dispatchers.Default)

    @Before
    fun setUp() {
        job = SupervisorJob()
        scope = CoroutineScope(job + Dispatchers.Default)
        context.filesDir.listFiles()?.forEach { it.deleteRecursively() }
        context.getSharedPreferences("mewo_hosted", 0).edit().clear().commit()
    }

    @After
    fun tearDown() {
        job.cancel()
    }

    @Test
    fun signOutClearsTheFeedCache() = runBlocking {
        val cache = FileHomeFeedCache(File(context.filesDir, "home-feeds"))
        val key = "$SERVER\nada"
        cache.write(key, HomeFeedPage())
        assertTrue(cache.read(key) != null)
        val client = GateClient()
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(SERVER)
        HostedLibraryStore(context, EpubOpener(context), sessions, client, scope)
        sessions.setSignedIn("ada", "token-a")
        withTimeout(5_000) { client.started("token-a") }
        sessions.signOut()
        withTimeout(5_000) {
            while (cache.read(key) != null) delay(20)
        }
        assertEquals(null, cache.read(key))
        client.respond("token-a", emptyList())
    }

    @Test
    fun switchingAccountsDoesNotLeakTheShelfOrSignOut() = runBlocking {
        val client = GateClient()
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(SERVER)
        val store = HostedLibraryStore(context, EpubOpener(context), sessions, client, scope)
        shelfCache().write(SERVER, "ada", listOf(book(PRIVATE, "Ada's diary", owner = "ada", mine = true)))
        sessions.setSignedIn("ada", "token-a")
        withTimeout(5_000) { client.started("token-a") }
        assertEquals(listOf(PRIVATE), store.library.value.books.map { it.id })

        client.expireToken = "token-a"
        sessions.setSignedIn("bea", "token-b")
        delay(250)
        val whileAIsInFlight = store.library.value.books.map { it.id }
        client.respond("token-a", listOf(book(PRIVATE, "Ada's diary", owner = "ada", mine = true)))
        delay(250)
        assertFalse(
            "B saw ${whileAIsInFlight} then ${store.library.value.books.map { it.id }} token=${sessions.session.value.token}",
            PRIVATE in whileAIsInFlight || sessions.session.value.token != "token-b",
        )
        withTimeout(5_000) { client.started("token-b") }
        client.respond("token-b", listOf(book(BEA, "Bea's book", owner = "bea", mine = true)))
        withTimeout(5_000) {
            while (store.library.value.books.none { it.id == BEA }) delay(20)
        }
        assertEquals("token-b", sessions.session.value.token)
        assertEquals("bea", sessions.session.value.username)
        assertEquals(listOf(BEA), store.library.value.books.map { it.id })
        assertFalse(store.library.value.books.any { it.id == PRIVATE })
    }

    @Test
    fun openingABookDuringRefreshDoesNotKeepRowsTheServerRemoved() = runBlocking {
        val client = GateClient()
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(SERVER)
        val store = HostedLibraryStore(context, EpubOpener(context), sessions, client, scope)
        val stale = book(STALE, "Removed", postCount = 12)
        val kept = book(KEPT, "Kept", postCount = 4)
        shelfCache().write(SERVER, "ada", listOf(stale, kept))
        sessions.setSignedIn("ada", "token-a")
        withTimeout(5_000) { client.started("token-a") }
        scope.launch { runCatching { store.feed(STALE) } }
        withTimeout(5_000) {
            while (store.library.value.books.find { it.id == STALE }?.postCount != 1) delay(20)
        }
        client.respond("token-a", listOf(kept))
        withTimeout(5_000) {
            while (store.loadState.value.isRefreshing()) delay(20)
        }
        val ids = store.library.value.books.map { it.id }
        assertFalse("kept stale rows $ids", STALE in ids)
        assertEquals(listOf(KEPT), ids)
        assertFalse(File(bookDir(STALE), "posts.json").exists())
    }

    @Test
    fun aFullDiskDoesNotFailASuccessfulRefresh() = runBlocking {
        File(context.filesDir, "hosted-shelves").apply {
            parentFile?.mkdirs()
            writeText("not a directory")
        }
        val client = GateClient()
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(SERVER)
        val store = HostedLibraryStore(context, EpubOpener(context), sessions, client, scope)
        sessions.setSignedIn("ada", "token-a")
        withTimeout(5_000) { client.started("token-a") }
        client.respond("token-a", listOf(book(KEPT, "Kept")))
        withTimeout(5_000) {
            while (store.loadState.value.isRefreshing() || store.loadState.value is LibraryLoading) delay(20)
        }
        assertTrue(
            "load state ${store.loadState.value}",
            store.loadState.value is LibraryLoaded,
        )
        assertEquals(KEPT, store.library.value.books.single().id)
    }

    private fun shelfCache() = ShelfCache(File(context.filesDir, "hosted-shelves"), json)

    private fun bookDir(id: String) = File(File(context.filesDir, "hosted"), id)

    private fun book(
        id: String,
        title: String,
        owner: String = "ada",
        mine: Boolean = true,
        postCount: Int = 1,
    ) = BookRecord(
        id = id,
        title = title,
        author = "Austen",
        handle = "austen",
        importedAt = 1,
        postCount = postCount,
        mine = mine,
        shared = false,
        ownerUsername = owner,
    )

    private class GateClient : HostedClient() {
        var expireToken: String? = null
        private val gates = ConcurrentHashMap<String, CompletableDeferred<LibrarySnapshot>>()

        suspend fun started(token: String) {
            while (gates[token] == null) delay(10)
        }

        fun respond(token: String, books: List<BookRecord>) {
            gates.getValue(token).complete(LibrarySnapshot(books))
        }

        override fun library(session: HostedSession): LibrarySnapshot {
            val token = session.token.orEmpty()
            val gate = CompletableDeferred<LibrarySnapshot>()
            gates[token] = gate
            return runBlocking { gate.await() }
        }

        override fun posts(session: HostedSession, id: String): List<FeedPost> {
            return listOf(FeedPost(id = "p", kind = PostKind.Body, text = "secret", chapter = "1"))
        }

        override fun downloadCover(session: HostedSession, id: String, dest: File): Boolean {
            if (session.token != null && session.token == expireToken) throw HostedException("expired", 401)
            return false
        }
    }

    companion object {
        private const val SERVER = "https://mewo.example"
        private const val PRIVATE = "11111111-1111-1111-1111-111111111111"
        private const val BEA = "22222222-2222-2222-2222-222222222222"
        private const val STALE = "33333333-3333-3333-3333-333333333333"
        private const val KEPT = "44444444-4444-4444-4444-444444444444"
    }
}
