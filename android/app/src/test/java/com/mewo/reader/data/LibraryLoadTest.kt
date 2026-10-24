package com.mewo.reader.data

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

class LibraryLoadTest {
    private val json = Json { ignoreUnknownKeys = true }

    @Test
    fun emptyCopyWaitsForASuccessfulFetch() {
        assertFalse(LibraryLoading.showEmpty())
        assertTrue(LibraryLoading.books().isEmpty())

        val none = reduceLibraryLoad(LibraryLoading, LibraryEvent.RefreshSucceeded(emptyList()))
        assertTrue(none.showEmpty())
        assertFalse(none.isRefreshing())

        val cached = reduceLibraryLoad(LibraryLoading, LibraryEvent.Cached(listOf(book("a"))))
        assertFalse(cached.showEmpty())
        assertTrue(cached.isRefreshing())
        val refreshing = reduceLibraryLoad(cached, LibraryEvent.RefreshStarted)
        assertTrue(refreshing is LibraryLoaded && refreshing.refreshing)
        assertEquals("a", refreshing.books().single().id)
    }

    @Test
    fun failedRefreshKeepsTheSavedShelf() {
        val saved = book("a", shared = true, mine = false, owner = "ada", progress = 4)
        val loaded = LibraryLoaded(listOf(saved), refreshing = true)
        val failed = reduceLibraryLoad(loaded, LibraryEvent.RefreshFailed("offline"))
        assertTrue(failed is LibraryFailed)
        assertEquals("offline", (failed as LibraryFailed).message)
        assertEquals(4, failed.books().single().progressIndex)
        assertEquals("ada", failed.books().single().ownerUsername)
        assertTrue(failed.books().single().shared)
        assertFalse(failed.books().single().mine)
        assertFalse(failed.showEmpty())

        val first = reduceLibraryLoad(LibraryLoading, LibraryEvent.RefreshFailed("offline"))
        assertTrue(first is LibraryFailed)
        assertNull((first as LibraryFailed).books)
        assertFalse(first.showEmpty())
        assertTrue(reduceLibraryLoad(first, LibraryEvent.RefreshStarted) is LibraryLoading)
    }

    @Test
    fun savedTextStaysAvailableUntilTheShelfSettles() {
        val id = "11111111-1111-1111-1111-111111111111"
        val other = "22222222-2222-2222-2222-222222222222"
        assertTrue(savedTextAllowed(LibraryLoading, id))
        assertTrue(savedTextAllowed(LibraryLoaded(listOf(book(id)), refreshing = true), id))
        assertTrue(savedTextAllowed(LibraryLoaded(listOf(book(other)), refreshing = true), id))
        assertTrue(savedTextAllowed(LibraryFailed("offline", null), id))
        assertFalse(savedTextAllowed(LibraryLoaded(listOf(book(other)), refreshing = false), id))
        assertTrue(savedTextAllowed(LibraryLoaded(listOf(book(id)), refreshing = false), id))
        assertFalse(savedTextAllowed(LibraryFailed("offline", listOf(book(other))), id))
        assertTrue(savedTextAllowed(LibraryFailed("offline", listOf(book(id))), id))
    }

    @Test
    fun dismissingACachedFailureReturnsToTheShelf() {
        val failed = LibraryFailed("offline", listOf(book("a")))
        val dismissed = reduceLibraryLoad(failed, LibraryEvent.Dismissed)
        assertTrue(dismissed is LibraryLoaded)
        assertFalse((dismissed as LibraryLoaded).refreshing)
        assertEquals("a", dismissed.books.single().id)
    }

    @Test
    fun shelfFilesArePerServerAndUsername() {
        val dir = Files.createTempDirectory("mewo-shelves").toFile()
        val cache = ShelfCache(dir, json)
        val ada = listOf(book("a", shared = false, mine = true, owner = "ada", progress = 8))
        val matt = listOf(book("b", shared = true, mine = false, owner = "matt", progress = 1))
        cache.write("https://mewo.example", "ada", ada)
        cache.write("https://mewo.example", "matt", matt)
        cache.write("http://other.example", "ada", listOf(book("c")))
        assertFalse(
            shelfCacheKey("https://mewo.example", "ada") == shelfCacheKey("https://mewo.example", "matt"),
        )
        assertFalse(
            shelfCacheKey("https://mewo.example", "ada") == shelfCacheKey("http://other.example", "ada"),
        )
        assertEquals("a", cache.read("https://mewo.example", "ada")!!.single().id)
        assertEquals(8, cache.read("https://mewo.example", "ada")!!.single().progressIndex)
        assertEquals("matt", cache.read("https://mewo.example", "matt")!!.single().ownerUsername)
        assertTrue(cache.read("https://mewo.example", "matt")!!.single().shared)
        assertEquals("c", cache.read("http://other.example", "ada")!!.single().id)
        cache.clear("https://mewo.example", "ada")
        assertNull(cache.read("https://mewo.example", "ada"))
        assertEquals("b", cache.read("https://mewo.example", "matt")!!.single().id)
        dir.deleteRecursively()
    }

    @Test
    fun savedPostsAreReusedUntilTheShapeChanges() {
        val dir = Files.createTempDirectory("mewo-posts").toFile()
        val file = File(dir, "posts.json")
        val posts = listOf(FeedPost(id = "p", kind = PostKind.Body, text = "Hello", chapter = "1"))
        file.writeText(json.encodeToString(FeedCache(version = 1, posts = posts)))
        assertEquals("Hello", readHostedPosts(file, json)!!.single().text)
        file.writeText(json.encodeToString(FeedCache(version = 99, posts = posts)))
        assertNull(readHostedPosts(file, json))
        dir.deleteRecursively()
    }

    private fun book(
        id: String,
        shared: Boolean = false,
        mine: Boolean = true,
        owner: String = "ada",
        progress: Int = 0,
    ) = BookRecord(
        id = id,
        title = id,
        author = "Austen",
        handle = "austen",
        importedAt = 1,
        postCount = 12,
        progressIndex = progress,
        mine = mine,
        shared = shared,
        ownerUsername = owner,
    )
}
