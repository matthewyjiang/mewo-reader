package com.mewo.reader.data

import com.mewo.reader.ui.components.emailSentNotice
import com.mewo.reader.ui.components.emailTrail
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class HostedEmailClientTest {
    @Test
    fun registerSendsEmailOnlyWhenGiven() = withServer { server ->
        server.enqueue(MockResponse().setBody("""{"token":"t","username":"ada"}"""))
        server.enqueue(MockResponse().setBody("""{"token":"t","username":"ada"}"""))
        val client = HostedClient()
        client.register(base(server), "ada", "password1", " ada@example.com ")
        val first = server.takeRequest()
        assertEquals("/v1/auth/register", first.path)
        assertEquals(
            """{"username":"ada","password":"password1","email":"ada@example.com"}""",
            first.body.readUtf8(),
        )
        client.register(base(server), "ada", "password1", "  ")
        assertEquals(
            """{"username":"ada","password":"password1"}""",
            server.takeRequest().body.readUtf8(),
        )
    }

    @Test
    fun requestResetPostsTheLoginWithoutASession() = withServer { server ->
        server.enqueue(MockResponse().setResponseCode(202))
        HostedClient().requestReset(base(server), " ada ")
        val request = server.takeRequest()
        assertEquals("POST", request.method)
        assertEquals("/v1/auth/reset/request", request.path)
        assertNull(request.getHeader("Authorization"))
        assertEquals("""{"login":"ada"}""", request.body.readUtf8())
    }

    @Test
    fun setEmailReportsWhetherALinkWentOut() = withServer { server ->
        server.enqueue(MockResponse().setResponseCode(202))
        server.enqueue(MockResponse().setResponseCode(204))
        val client = HostedClient()
        assertTrue(client.setEmail(session(server), "ada@example.com", "password1"))
        val put = server.takeRequest()
        assertEquals("PUT", put.method)
        assertEquals("/v1/me/email", put.path)
        assertEquals("Bearer token", put.getHeader("Authorization"))
        assertEquals(
            """{"email":"ada@example.com","currentPassword":"password1"}""",
            put.body.readUtf8(),
        )
        assertFalse(client.setEmail(session(server), null, "password1"))
        assertEquals(
            """{"email":null,"currentPassword":"password1"}""",
            server.takeRequest().body.readUtf8(),
        )
    }

    @Test
    fun setEmailRaisesTheServerMessage() = withServer { server ->
        server.enqueue(MockResponse().setResponseCode(403).setBody("""{"error":"Wrong password."}"""))
        val err = runCatching {
            HostedClient().setEmail(session(server), "ada@example.com", "nope")
        }.exceptionOrNull() as HostedException
        assertEquals("Wrong password.", err.message)
        assertEquals(403, err.code)
    }

    @Test
    fun meReadsEmailFieldsAndOlderServersOmitThem() = withServer { server ->
        server.enqueue(
            MockResponse().setBody(
                """{"username":"ada","email":null,"pendingEmail":"new@example.com","passwordReset":true}""",
            ),
        )
        server.enqueue(MockResponse().setBody("""{"username":"ada"}"""))
        val client = HostedClient()
        val me = client.me(session(server))
        assertNull(me.email)
        assertEquals("new@example.com", me.pendingEmail)
        assertTrue(me.passwordReset)
        val old = client.me(session(server))
        assertNull(old.pendingEmail)
        assertFalse(old.passwordReset)
    }

    @Test
    fun emailTrailPrefersPending() {
        assertEquals("Add", emailTrail(Me()))
        assertEquals("ada@example.com", emailTrail(Me(email = "ada@example.com")))
        assertEquals(
            "Check inbox",
            emailTrail(Me(email = "ada@example.com", pendingEmail = "new@example.com")),
        )
        assertEquals("Check new@example.com for a link.", emailSentNotice(" new@example.com "))
    }

    private fun withServer(block: (MockWebServer) -> Unit) {
        val server = MockWebServer()
        server.start()
        try {
            block(server)
        } finally {
            server.shutdown()
        }
    }

    private fun base(server: MockWebServer) = server.url("/").toString().trimEnd('/')

    private fun session(server: MockWebServer) = HostedSession(
        baseUrl = base(server),
        username = "ada",
        token = "token",
    )
}
