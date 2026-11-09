package com.mewo.reader.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class HostedSessionStoreTest {
    private val context = RuntimeEnvironment.getApplication()

    @Before
    fun setUp() {
        context.getSharedPreferences("mewo_hosted", 0).edit().clear().commit()
    }

    @Test
    fun expirePersistsAcrossInstances() {
        val store = HostedSessionStore(context)
        store.setBaseUrl("https://mewo.example/")
        store.setSignedIn("ada", "token-a")
        store.expire()

        assertFalse(store.session.value.signedIn)
        assertNull(store.session.value.username)
        assertNull(store.session.value.token)
        assertEquals(OutboxAccount("https://mewo.example", "ada"), store.session.value.expired)
        assertEquals(store.session.value, HostedSessionStore(context).session.value)
    }

    @Test
    fun setSignedInAndSignOutClearExpired() {
        val store = HostedSessionStore(context)
        store.setBaseUrl("https://mewo.example")
        store.setSignedIn("ada", "token-a")
        store.expire()
        store.setSignedIn("ada", "token-b")
        assertNull(store.session.value.expired)
        assertEquals("token-b", store.session.value.token)
        assertNull(HostedSessionStore(context).session.value.expired)

        store.expire()
        assertEquals(OutboxAccount("https://mewo.example", "ada"), store.session.value.expired)
        store.signOut()
        assertNull(store.session.value.expired)
        val restored = HostedSessionStore(context)
        assertNull(restored.session.value.expired)
        assertNull(restored.session.value.token)
        assertEquals("https://mewo.example", restored.session.value.baseUrl)
    }

    @Test
    fun editingTheUrlDoesNotClearExpired() {
        val store = HostedSessionStore(context)
        store.setBaseUrl("https://mewo.example")
        store.setSignedIn("ada", "token-a")
        store.expire()
        store.setBaseUrl("https://other.example")

        val again = HostedSessionStore(context)
        assertEquals("https://other.example", again.session.value.baseUrl)
        assertEquals(OutboxAccount("https://mewo.example", "ada"), again.session.value.expired)
        assertFalse(again.session.value.signedIn)
    }

    @Test
    fun signingInRecordsTheDiskOwnerUntilADifferentAccount() {
        val store = HostedSessionStore(context)
        store.setBaseUrl("https://mewo.example")
        store.setSignedIn("ada", "token-a")
        assertEquals(OutboxAccount("https://mewo.example", "ada"), store.diskOwner())
        store.setSignedIn("bea", "token-b")
        assertEquals(OutboxAccount("https://mewo.example", "ada"), store.diskOwner())
        assertEquals(OutboxAccount("https://mewo.example", "ada"), HostedSessionStore(context).diskOwner())
    }

    @Test
    fun setSignedInDoesNotClearTheDiskOwner() {
        val store = HostedSessionStore(context)
        val ada = OutboxAccount("https://mewo.example", "ada")
        store.setBaseUrl("https://mewo.example")
        store.setDiskOwner(ada)
        store.setSignedIn("bea", "token-b")
        store.expire()

        assertEquals(OutboxAccount("https://mewo.example", "bea"), store.session.value.expired)
        assertEquals(ada, store.diskOwner())
        assertEquals(ada, HostedSessionStore(context).diskOwner())

        store.signOut()
        assertNull(store.session.value.expired)
        assertEquals(ada, HostedSessionStore(context).diskOwner())
    }
}
