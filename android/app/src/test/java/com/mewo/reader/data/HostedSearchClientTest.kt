package com.mewo.reader.data

import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class HostedSearchClientTest {
    @Test
    fun aBlankQueryOmitsQ() {
        val server = MockWebServer()
        repeat(4) {
            server.enqueue(MockResponse().setBody("""{"type":"books","items":[],"nextCursor":null}"""))
        }
        server.start()
        try {
            val session = HostedSession(
                baseUrl = server.url("/").toString().trimEnd('/'),
                username = "ada",
                token = "token",
            )
            val client = HostedClient()
            client.search(session, "  ", SearchKind.Books, null)
            val blank = server.takeRequest().path
            assertEquals("/v1/search?type=books", blank)
            assertFalse(blank!!.contains("q="))

            client.search(session, "em", SearchKind.Books, null)
            assertEquals("/v1/search?q=em&type=books", server.takeRequest().path)

            client.search(session, "em", SearchKind.Books, "c1")
            assertEquals("/v1/search?q=em&type=books&cursor=c1", server.takeRequest().path)

            client.search(session, "a b", SearchKind.Accounts, null)
            assertEquals("/v1/search?q=a%20b&type=accounts", server.takeRequest().path)
        } finally {
            server.shutdown()
        }
    }
}
