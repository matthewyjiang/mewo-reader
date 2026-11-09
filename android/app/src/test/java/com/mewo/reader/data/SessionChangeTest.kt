package com.mewo.reader.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionChangeTest {
    @Test
    fun a401OnTheCurrentTokenIsExpired() {
        val previous = HostedSession(baseUrl = SERVER, username = "ada", token = "t")
        val next = HostedSession(baseUrl = SERVER, expired = ada)
        assertEquals(SessionChange.Expired, decide(previous, next))
    }

    @Test
    fun signingBackIntoTheExpiredAccountKeeps() {
        val previous = HostedSession(baseUrl = SERVER, expired = ada)
        val next = HostedSession(baseUrl = SERVER, username = "ada", token = "t2")
        assertEquals(SessionChange.Keep, decide(previous, next))
    }

    @Test
    fun anotherAccountOnTheSameServerIsOtherUser() {
        val expired = HostedSession(baseUrl = SERVER, expired = ada)
        val bea = HostedSession(baseUrl = SERVER, username = "bea", token = "t")
        assertEquals(SessionChange.OtherUser(ada), decide(expired, bea))

        val signedIn = HostedSession(baseUrl = SERVER, username = "ada", token = "a")
        assertEquals(SessionChange.OtherUser(ada), decide(signedIn, bea))
    }

    @Test
    fun anotherServerWipes() {
        val expired = HostedSession(baseUrl = SERVER, expired = ada)
        val next = HostedSession(baseUrl = OTHER, username = "ada", token = "t")
        assertEquals(SessionChange.Wipe(ada), decide(expired, next))

        val signedIn = HostedSession(baseUrl = SERVER, username = "ada", token = "t")
        val moved = HostedSession(baseUrl = OTHER, username = "ada", token = "t")
        assertEquals(SessionChange.Wipe(ada), decide(signedIn, moved))
    }

    @Test
    fun explicitSignOutWipes() {
        val previous = HostedSession(baseUrl = SERVER, username = "ada", token = "t")
        val next = HostedSession(baseUrl = SERVER)
        assertEquals(SessionChange.Wipe(ada), decide(previous, next))
    }

    @Test
    fun editingTheUrlWhileExpiredKeeps() {
        val previous = HostedSession(baseUrl = SERVER, expired = ada)
        val next = HostedSession(baseUrl = OTHER, expired = ada)
        assertEquals(SessionChange.Keep, decide(previous, next))
    }

    @Test
    fun signedInWithNoUsernameThenSignedOutWipes() {
        val previous = HostedSession(baseUrl = SERVER, token = "t")
        val next = HostedSession(baseUrl = SERVER)
        assertTrue(previous.signedIn)
        assertNull(previous.filesOwner())
        assertEquals(SessionChange.Wipe(null), decide(previous, next))
    }

    @Test
    fun tokenRotationKeeps() {
        val previous = HostedSession(baseUrl = SERVER, username = "ada", token = "t1")
        val next = HostedSession(baseUrl = SERVER, username = "ada", token = "t2")
        assertEquals(SessionChange.Keep, decide(previous, next))
    }

    @Test
    fun signingInWithNothingSavedKeeps() {
        val previous = HostedSession(baseUrl = SERVER)
        val next = HostedSession(baseUrl = SERVER, username = "ada", token = "t")
        assertEquals(SessionChange.Keep, decide(previous, next))
    }

    @Test
    fun leavingASignedInSessionWithNoUsernameForAnAccountWipes() {
        val bare = HostedSession(baseUrl = SERVER, token = "t")
        val bea = HostedSession(baseUrl = SERVER, username = "bea", token = "b")
        val sessionDelta = sessionChange(null, bare, bea)
        assertEquals(SessionChange.Wipe(null), sessionDelta)
        val cleanup = fileCleanup(sessionDelta, sessionChange(OutboxAccount(SERVER, "bea"), bare, bea))
        assertTrue(cleanup.wipeHosted)
        assertEquals(false, cleanup.clearSocial)
    }

    @Test
    fun aStaleDiskOwnerDoesNotHideTheAccountThatJustLeft() {
        val bea = OutboxAccount(SERVER, "bea")
        val beaSession = HostedSession(baseUrl = SERVER, username = "bea", token = "t")
        val signedOut = HostedSession(baseUrl = SERVER)
        val cleanup = fileCleanup(
            sessionChange(null, beaSession, signedOut),
            sessionChange(ada, beaSession, signedOut),
        )
        assertEquals(listOf(ada, bea), cleanup.accounts)
        assertTrue(cleanup.wipeHosted)
        assertEquals(false, cleanup.clearSocial)
    }

    @Test
    fun aWipeIsNotDowngradedToASocialDelete() {
        val bea = OutboxAccount(SERVER, "bea")
        val cleanup = fileCleanup(SessionChange.Wipe(bea), SessionChange.OtherUser(ada))
        assertEquals(listOf(ada, bea), cleanup.accounts)
        assertTrue(cleanup.wipeHosted)
        assertEquals(false, cleanup.clearSocial)
    }

    @Test
    fun diskOwnerStillDropsThePreviousAccountAfterTheSessionForgotThem() {
        val bea = HostedSession(baseUrl = SERVER, username = "bea", token = "t")
        assertEquals(SessionChange.OtherUser(ada), sessionChange(ada, bea, bea))

        val expiredBea = HostedSession(baseUrl = SERVER, expired = OutboxAccount(SERVER, "bea"))
        assertEquals(SessionChange.OtherUser(ada), sessionChange(ada, expiredBea, expiredBea))
    }

    private fun decide(previous: HostedSession, next: HostedSession) =
        sessionChange(previous.filesOwner(), previous, next)

    companion object {
        private const val SERVER = "https://mewo.example"
        private const val OTHER = "https://other.example"
        private val ada = OutboxAccount(SERVER, "ada")
    }
}
