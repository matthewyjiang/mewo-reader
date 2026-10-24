package com.mewo.reader.data

import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HostedSocialClientTest {
    private val book = """
        {"id":"b1","title":"Emma","author":"Austen","handle":"austen","importedAt":1,
         "postCount":3,"progressIndex":0,"isSample":false,"mine":true,"shared":true,
         "ownerUsername":"ada"}
    """.trimIndent().replace("\n", "")

    private val post = """{"id":"0","kind":"Body","text":"A line.","chapter":"One"}"""

    @Test
    fun searchAccountsBooksAndPosts() {
        val accounts = client(
            """{"type":"accounts","items":[{"username":"ada","mine":true}],"nextCursor":"c1"}""",
        ) { session, api -> api.search(session, "ada", SearchKind.Accounts, null) }
        val accountPage = accounts.second as SearchPage.Accounts
        assertEquals("/v1/search?q=ada&type=accounts", accounts.first)
        assertEquals("ada", accountPage.items.single().username)
        assertEquals("c1", accountPage.nextCursor)

        val books = client(
            """{"type":"books","items":[$book],"nextCursor":null}""",
        ) { session, api -> api.search(session, "em", SearchKind.Books, "c1") }
        assertEquals("/v1/search?q=em&type=books&cursor=c1", books.first)
        assertEquals("Emma", (books.second as SearchPage.Books).items.single().title)

        val posts = client(
            """{"type":"posts","items":[{"book":$book,"post":$post}],"nextCursor":null}""",
        ) { session, api -> api.search(session, "line", SearchKind.Posts, null) }
        assertEquals("A line.", (posts.second as SearchPage.Posts).items.single().post.text)
    }

    @Test
    fun profilePostsRepostsAndBooks() {
        val user = client(
            """{"username":"ada","joinedAt":1609459200000,"postCount":2,"repostCount":1,"mine":true}""",
        ) { session, api -> api.user(session, "ada") }
        assertEquals("/v1/users/ada", user.first)
        assertEquals("Joined Jan 2021", joinedLabel(user.second.joinedAt))
        assertEquals(2, user.second.postCount)

        val posts = client(
            """{"items":[{"book":$book,"post":$post,"comment":{"id":"c1","username":"ada","text":"Nice","createdAt":5,"mine":true}}],"nextCursor":"n"}""",
        ) { session, api -> api.userPosts(session, "ada", null) }
        assertEquals("/v1/users/ada/posts", posts.first)
        assertEquals("Nice", posts.second.items.single().comment.text)
        assertEquals("n", posts.second.nextCursor)

        val reposts = client(
            """{"items":[{"book":$book,"post":$post,"repostedAt":6}],"nextCursor":null}""",
        ) { session, api -> api.userReposts(session, "ada", "n") }
        assertEquals("/v1/users/ada/reposts?cursor=n", reposts.first)
        assertEquals(6, reposts.second.items.single().repostedAt)

        val books = client(
            """{"items":[$book]}""",
        ) { session, api -> api.userBooks(session, "ada") }
        assertEquals("/v1/users/ada/books", books.first)
        assertTrue(books.second.items.single().shared)
    }

    @Test
    fun commentIndexKeepsRepostsAndTheFeedDecodesBothKinds() {
        val index = client(
            """{"counts":{"0":2},"mine":["0"],"reposts":{"0":3},"reposted":["0"]}""",
        ) { session, api -> api.commentIndex(session, "b1") }
        assertEquals("/v1/books/b1/comments", index.first)
        assertEquals(3, index.second.repostCount("0"))
        assertTrue(index.second.hasReposted("0"))

        val feed = client(
            """{"items":[
              {"type":"repost","actor":{"username":"byron"},"book":$book,"post":$post,"at":10},
              {"type":"comment","actor":{"username":"ada"},"book":$book,"post":$post,
               "comment":{"id":"c1","username":"ada","text":"Nice","createdAt":9,"mine":true},"at":9}
            ],"nextCursor":"f"}""",
        ) { session, api -> api.homeFeed(session, null) }
        assertEquals("/v1/feed", feed.first)
        assertTrue(feed.second.items[0] is HomeFeedItem.Repost)
        assertTrue(feed.second.items[1] is HomeFeedItem.Comment)
        assertEquals("f", feed.second.nextCursor)
    }

    @Test
    fun postsReportsByteProgressWhenTheLengthIsKnown() {
        val body = """{"posts":[{"id":"p","kind":"Body","text":"Hello","chapter":"1"}]}"""
        val server = MockWebServer()
        server.enqueue(MockResponse().setBody(body))
        server.start()
        try {
            val session = HostedSession(
                baseUrl = server.url("/").toString().trimEnd('/'),
                username = "ada",
                token = "token",
            )
            val seen = mutableListOf<Float?>()
            val posts = HostedClient().posts(session, "b1") { seen.add(it) }
            assertEquals("Hello", posts.single().text)
            assertEquals("/v1/books/b1/posts", server.takeRequest().path)
            assertTrue(seen.isNotEmpty())
            assertTrue(seen.all { it != null })
            assertEquals(0f, seen.first())
            assertEquals(1f, seen.last())
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun postsStaysIndeterminateWithoutALength() {
        val body = """{"posts":[{"id":"p","kind":"Body","text":"Hello","chapter":"1"}]}"""
        val server = MockWebServer()
        server.enqueue(MockResponse().setChunkedBody(body, 8))
        server.start()
        try {
            val session = HostedSession(
                baseUrl = server.url("/").toString().trimEnd('/'),
                username = "ada",
                token = "token",
            )
            val seen = mutableListOf<Float?>()
            HostedClient().posts(session, "b1") { seen.add(it) }
            assertTrue(seen.isNotEmpty())
            assertTrue(seen.all { it == null })
        } finally {
            server.shutdown()
        }
    }

    @Test
    fun setRepostUsesPutAndDelete() {
        val on = client(
            """{"reposts":1,"reposted":true}""",
        ) { session, api -> api.setRepost(session, "b1", "0", true) }
        assertEquals("PUT", on.third)
        assertEquals("/v1/books/b1/posts/0/repost", on.first)
        assertTrue(on.second.reposted)

        val off = client(
            """{"reposts":0,"reposted":false}""",
        ) { session, api -> api.setRepost(session, "b1", "0", false) }
        assertEquals("DELETE", off.third)
        assertEquals(0, off.second.reposts)
    }

    private fun <T> client(body: String, call: (HostedSession, HostedClient) -> T): Triple<String, T, String?> {
        val server = MockWebServer()
        server.enqueue(MockResponse().setBody(body))
        server.start()
        try {
            val session = HostedSession(
                baseUrl = server.url("/").toString().trimEnd('/'),
                username = "ada",
                token = "token",
            )
            val value = call(session, HostedClient())
            val request = server.takeRequest()
            assertEquals("Bearer token", request.getHeader("Authorization"))
            return Triple(request.path ?: "", value, request.method)
        } finally {
            server.shutdown()
        }
    }
}
