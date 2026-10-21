package com.mewo.reader.data

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class HostedCatalogClientTest {
    private val context = RuntimeEnvironment.getApplication()
    private var job = SupervisorJob()
    private var scope = CoroutineScope(job + Dispatchers.Default)

    @Before
    fun setUp() {
        job = SupervisorJob()
        scope = CoroutineScope(job + Dispatchers.Default)
        context.getSharedPreferences("mewo_hosted", 0).edit().clear().commit()
    }

    @After
    fun tearDown() {
        job.cancel()
    }

    @Test
    fun aBlankQueryOmitsQ() {
        val server = MockWebServer()
        server.enqueue(MockResponse().setBody("""{"items":[],"nextPage":null}"""))
        server.enqueue(MockResponse().setBody("""{"items":[],"nextPage":2}"""))
        server.start()
        try {
            val session = session(server)
            val client = HostedClient()
            client.catalog(session, "  ", 1)
            assertEquals("/v1/catalog/search?page=1", server.takeRequest().path)
            client.catalog(session, "emma", 2)
            assertEquals("/v1/catalog/search?page=2&q=emma", server.takeRequest().path)
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun workDetailDecodesAFlattenedRequest() {
        val body = """
            {"workKey":"/works/OL123W","info":null,"status":{
              "request":{
                "id":"r1","workKey":"/works/OL123W","title":"Dune","author":"Herbert",
                "year":1965,"coverId":9,"state":"requested","progress":0,
                "error":"No release yet.","bookId":null,"requester":"ada",
                "createdAt":1,"updatedAt":2,"attempts":3,"maxAttempts":8,
                "nextCheckAt":99,"releaseTitle":"Dune 1965","releaseSize":1200,
                "phase":"waiting"
              },
              "onServer":null,"active":1,"cap":5,
              "otherRequests":[{"id":"r2","workKey":"/works/OL123W","title":"Dune","state":"requested","requester":"bea"}]
            }}
        """.trimIndent().replace("\n", "")
        val server = MockWebServer()
        server.enqueue(MockResponse().setBody(body))
        server.start()
        try {
            val detail = HostedClient().catalogWork(session(server), "OL123W")
            assertEquals("/v1/catalog/works/OL123W", server.takeRequest().path)
            val request = detail.status.request!!
            assertEquals("waiting", request.phase)
            assertEquals(3, request.attempts)
            assertEquals(8, request.maxAttempts)
            assertEquals(99L, request.nextCheckAt)
            assertEquals("Dune 1965", request.releaseTitle)
            assertEquals(1200L, request.releaseSize)
            assertEquals("r1", request.asBookRequest().id)
            assertEquals("bea", detail.status.otherRequests.single().requester)
            assertEquals(5, detail.status.cap)
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun requestCancelAndRetryUseTheRequestRoutes() {
        val created = """
            {"id":"r1","workKey":"/works/OL123W","title":"Dune","author":"Herbert",
             "state":"requested","progress":0,"requester":"ada","createdAt":1,"updatedAt":1}
        """.trimIndent().replace("\n", "")
        val server = MockWebServer()
        server.enqueue(MockResponse().setBody(created))
        server.enqueue(MockResponse().setResponseCode(204))
        server.enqueue(MockResponse().setBody(created))
        server.start()
        try {
            val client = HostedClient()
            val session = session(server)
            val work = CatalogWork(
                workKey = "/works/OL123W",
                title = "Dune",
                author = "Herbert",
                year = 1965,
            )
            assertEquals("r1", client.requestBook(session, work).id)
            val post = server.takeRequest()
            assertEquals("POST", post.method)
            assertEquals("/v1/requests", post.path)
            assertTrue(post.body.readUtf8().contains(""""workKey":"/works/OL123W""""))

            client.cancelRequest(session, "r1")
            val delete = server.takeRequest()
            assertEquals("DELETE", delete.method)
            assertEquals("/v1/requests/r1", delete.path)

            assertEquals("r1", client.retryRequest(session, "r1").id)
            val retry = server.takeRequest()
            assertEquals("POST", retry.method)
            assertEquals("/v1/requests/r1/retry", retry.path)
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun catalogIsNotSentToSearch() {
        val server = MockWebServer()
        server.start()
        try {
            var thrown = false
            try {
                HostedClient().search(session(server), "emma", SearchKind.Catalog, null)
            } catch (err: IllegalStateException) {
                thrown = true
                assertEquals("Catalog does not use /v1/search.", err.message)
            }
            assertTrue(thrown)
            assertEquals(0, server.requestCount)
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun aForbiddenMeTurnsTheCatalogOff() = runBlocking {
        val server = scripted { path ->
            if (path.startsWith("/v1/me")) {
                MockResponse().setResponseCode(403).setBody("""{"error":"Requests are off."}""")
            } else {
                MockResponse().setResponseCode(500)
            }
        }
        try {
            val store = store(server)
            awaitAccess(store, CatalogAccess.Off)
            assertEquals("/v1/me", server.takeRequest().path)
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun canRequestFalseTurnsTheCatalogOff() = runBlocking {
        val server = scripted { path ->
            if (path.startsWith("/v1/me")) {
                MockResponse().setBody("""{"username":"ada","isAdmin":false,"canRequest":false}""")
            } else {
                MockResponse().setResponseCode(500)
            }
        }
        try {
            val store = store(server)
            awaitAccess(store, CatalogAccess.Off)
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun aForbiddenCatalogCallTurnsTheCatalogOff() = runBlocking {
        val server = scripted { path ->
            when {
                path.startsWith("/v1/me") ->
                    MockResponse().setBody("""{"username":"ada","isAdmin":false,"canRequest":true}""")
                path.startsWith("/v1/catalog/search") ->
                    MockResponse().setResponseCode(403).setBody("""{"error":"Requests are off."}""")
                else -> MockResponse().setResponseCode(500)
            }
        }
        try {
            val store = store(server)
            awaitAccess(store, CatalogAccess.On)
            var thrown: CatalogException? = null
            try {
                store.page("   ", 1)
            } catch (err: CatalogException) {
                thrown = err
            }
            val failure = thrown ?: error("catalog 403 should throw")
            assertEquals(CatalogException.Reason.Off, failure.reason)
            assertEquals("Requests are off.", failure.message)
            assertFalse(failure.message.contains("HTTP"))
            assertFalse(failure.message.contains("Server returned"))
            assertEquals(CatalogAccess.Off, store.access.value)
            val path = generateSequence { server.takeRequest(100, TimeUnit.MILLISECONDS) }
                .map { it.path }
                .first { it?.startsWith("/v1/catalog/search") == true }
            assertEquals("/v1/catalog/search?page=1", path)
        } finally {
            server.shutdown()
        }
    }

    private fun scripted(handler: (String) -> MockResponse): MockWebServer {
        val server = MockWebServer()
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                return handler(request.path.orEmpty())
            }
        }
        server.start()
        return server
    }

    private fun store(server: MockWebServer): HostedCatalogStore {
        val sessions = HostedSessionStore(context)
        sessions.setBaseUrl(server.url("/").toString().trimEnd('/'))
        sessions.setSignedIn("ada", "token")
        return HostedCatalogStore(
            sessions,
            HostedClient(),
            MutableStateFlow(BackendKind.Hosted),
            scope,
        )
    }

    private suspend fun awaitAccess(store: CatalogStore, expected: CatalogAccess) {
        withTimeout(5_000) {
            while (store.access.value != expected) delay(20)
        }
    }

    private fun session(server: MockWebServer) = HostedSession(
        baseUrl = server.url("/").toString().trimEnd('/'),
        username = "ada",
        token = "token",
    )
}
