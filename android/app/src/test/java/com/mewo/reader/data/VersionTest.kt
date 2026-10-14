package com.mewo.reader.data

import com.mewo.reader.ui.settings.formatAppVersion
import com.mewo.reader.ui.settings.formatServerVersion
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.fail
import org.junit.Test

class VersionTest {
    @Test
    fun appVersionIncludesCodeAndSkipsABlankCommit() {
        assertEquals("0.1.1 (1)", formatAppVersion("0.1.1", 1, null))
        assertEquals("0.1.1 (1)", formatAppVersion("0.1.1", 1, "  "))
        assertEquals("0.1.1 (1, abc1234)", formatAppVersion("0.1.1", 1, "abc1234"))
    }

    @Test
    fun serverVersionMatchesTheWebShape() {
        assertEquals("0.1.2", formatServerVersion("0.1.2", null))
        assertEquals("0.1.2 (def5678)", formatServerVersion("0.1.2", "def5678"))
    }

    @Test
    fun versionRouteIsPublic() {
        val server = MockWebServer()
        server.enqueue(
            MockResponse().setBody("""{"version":"0.1.2","commit":"def5678"}"""),
        )
        server.start()
        try {
            val info = HostedClient().version(server.url("/").toString())
            val request = server.takeRequest()
            assertEquals("GET", request.method)
            assertEquals("/v1/version", request.path)
            assertNull(request.getHeader("Authorization"))
            assertEquals("0.1.2", info.version)
            assertEquals("def5678", info.commit)
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun versionFailureThrows() {
        val server = MockWebServer()
        server.enqueue(MockResponse().setResponseCode(500))
        server.start()
        try {
            HostedClient().version(server.url("/").toString())
            fail("expected a server error")
        } catch (err: HostedException) {
            assertEquals(500, err.code)
        } finally {
            server.shutdown()
        }
    }
}
