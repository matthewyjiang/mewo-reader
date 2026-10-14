package com.mewo.reader.data

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class HostedSharingTest {
    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = false
    }

    @Test
    fun bookRecordKeepsShareFields() {
        val book = json.decodeFromString<BookRecord>(
            """
            {
              "id": "11111111-1111-1111-1111-111111111111",
              "title": "Pride and Prejudice",
              "author": "Austen",
              "handle": "austen",
              "importedAt": 5,
              "postCount": 12,
              "progressIndex": 2,
              "isSample": false,
              "mine": false,
              "shared": true,
              "ownerUsername": "ada",
              "extra": "ignored"
            }
            """.trimIndent(),
        )
        assertFalse(book.mine)
        assertTrue(book.shared)
        assertEquals("ada", book.ownerUsername)
        assertEquals(12, book.postCount)
    }

    @Test
    fun olderBookRecordDefaultsShareFields() {
        val snap = json.decodeFromString<LibrarySnapshot>(
            """
            {"books":[{
              "id": "1",
              "title": "Untitled",
              "author": "Unknown",
              "handle": "author",
              "importedAt": 1
            }]}
            """.trimIndent(),
        )
        val book = snap.books.single()
        assertTrue(book.mine)
        assertFalse(book.shared)
        assertNull(book.ownerUsername)
    }

    @Test
    fun nullOwnerUsernameDecodes() {
        val book = json.decodeFromString<BookRecord>(
            """
            {
              "id": "1",
              "title": "Untitled",
              "author": "Unknown",
              "handle": "author",
              "importedAt": 1,
              "ownerUsername": null
            }
            """.trimIndent(),
        )
        assertNull(book.ownerUsername)
        assertFalse(book.shared)
    }

    @Test
    fun patchBodySendsOnlyShared() {
        assertEquals("""{"shared":true}""", json.encodeToString(PatchBook(shared = true)))
        assertEquals("""{"shared":false}""", json.encodeToString(PatchBook(shared = false)))
        assertEquals("""{"progressIndex":4}""", json.encodeToString(PatchBook(progressIndex = 4)))
    }

    @Test
    fun setSharedPatchesOwnerFlag() {
        val server = MockWebServer()
        server.enqueue(MockResponse().setBody(bookJson(shared = true, owner = "matt")))
        server.start()
        try {
            val session = HostedSession(
                baseUrl = server.url("/").toString(),
                username = "matt",
                token = "token",
            )
            val book = HostedClient().setShared(session, BOOK_ID, true)
            val request = server.takeRequest()
            assertEquals("PATCH", request.method)
            assertEquals("/v1/books/$BOOK_ID", request.path)
            assertEquals("Bearer token", request.getHeader("Authorization"))
            assertEquals("""{"shared":true}""", request.body.readUtf8())
            assertTrue(book.shared)
            assertEquals("matt", book.ownerUsername)
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun shareToggleFlipsThenKeepsServerOwner() = runTest {
        val books = mutableListOf(book())
        val toggle = toggle(books)
        var patched: Boolean? = null
        toggle.setShared(BOOK_ID, true) {
            patched = true
            assertTrue(books.single().shared)
            book(shared = true, owner = "matt")
        }
        assertEquals(true, patched)
        assertTrue(books.single().shared)
        assertEquals("matt", books.single().ownerUsername)
    }

    @Test
    fun shareToggleRollsBackWhenPatchFails() = runTest {
        val books = mutableListOf(book(shared = true, owner = "matt"))
        val toggle = toggle(books)
        try {
            toggle.setShared(BOOK_ID, false) {
                error("Only the person who added this book can change sharing.")
            }
            fail("expected the patch to fail")
        } catch (err: IllegalStateException) {
            assertEquals(
                "Only the person who added this book can change sharing.",
                err.message,
            )
        }
        assertTrue(books.single().shared)
    }

    @Test
    fun shareToggleRejectsNonOwnerBeforePatch() = runTest {
        val books = mutableListOf(book(mine = false, shared = true, owner = "ada"))
        val toggle = toggle(books)
        var patched = false
        try {
            toggle.setShared(BOOK_ID, false) {
                patched = true
                book(mine = false, shared = false, owner = "ada")
            }
            fail("expected a non-owner to be rejected")
        } catch (err: IllegalStateException) {
            assertEquals(
                "Only the person who added this book can change sharing.",
                err.message,
            )
        }
        assertFalse(patched)
        assertTrue(books.single().shared)
    }

    @Test
    fun olderFailureDoesNotUndoANewerToggle() = runTest {
        val books = mutableListOf(book())
        val toggle = toggle(books)
        val first = CompletableDeferred<BookRecord>()
        val errors = mutableListOf<String>()
        val older = launch {
            try {
                toggle.setShared(BOOK_ID, true) { first.await() }
            } catch (err: IllegalStateException) {
                errors.add(err.message.orEmpty())
            }
        }
        advanceUntilIdle()
        assertTrue(books.single().shared)

        val newer = launch {
            toggle.setShared(BOOK_ID, false) { book(shared = false, owner = "matt") }
        }
        advanceUntilIdle()
        assertFalse(books.single().shared)

        first.completeExceptionally(IllegalStateException("nope"))
        advanceUntilIdle()
        older.join()
        newer.join()
        assertEquals(listOf("nope"), errors)
        assertFalse(books.single().shared)
        assertEquals("matt", books.single().ownerUsername)
    }

    private fun toggle(books: MutableList<BookRecord>): HostedShareToggle {
        return HostedShareToggle(
            books = { books.toList() },
            replace = { next ->
                val index = books.indexOfFirst { it.id == next.id }
                if (index >= 0) books[index] = next else books.add(next)
            },
        )
    }

    private fun book(
        shared: Boolean = false,
        mine: Boolean = true,
        owner: String? = null,
    ): BookRecord {
        return BookRecord(
            id = BOOK_ID,
            title = "Pride and Prejudice",
            author = "Austen",
            handle = "austen",
            importedAt = 1,
            mine = mine,
            shared = shared,
            ownerUsername = owner,
        )
    }

    private fun bookJson(shared: Boolean, owner: String): String {
        return """
            {
              "id": "$BOOK_ID",
              "title": "Pride and Prejudice",
              "author": "Austen",
              "handle": "austen",
              "importedAt": 1,
              "mine": true,
              "shared": $shared,
              "ownerUsername": "$owner"
            }
        """.trimIndent()
    }

    private companion object {
        const val BOOK_ID = "11111111-1111-1111-1111-111111111111"
    }
}
