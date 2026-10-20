package com.mewo.reader.data

import com.mewo.reader.ui.components.showCreateAccount
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class InstanceTest {
    @Test
    fun instanceRouteIsPublic() {
        val server = MockWebServer()
        server.enqueue(MockResponse().setBody("""{"allowSignup":false}"""))
        server.start()
        try {
            val info = HostedClient().instance(server.url("/").toString())
            val request = server.takeRequest()
            assertEquals("GET", request.method)
            assertEquals("/v1/instance", request.path)
            assertNull(request.getHeader("Authorization"))
            assertFalse(info.allowSignup)
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun unknownOrOpenKeepsTheCreateLink() {
        assertTrue(showCreateAccount(null))
        assertTrue(showCreateAccount(true))
    }

    @Test
    fun closedHidesTheCreateLink() {
        assertFalse(showCreateAccount(false))
    }
}
