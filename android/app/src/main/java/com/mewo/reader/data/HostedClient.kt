package com.mewo.reader.data

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.asRequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import java.io.File
import java.io.IOException
import java.util.concurrent.TimeUnit

class HostedException(message: String, val code: Int = 0) : Exception(message)

@Serializable
data class AuthRequest(
    val username: String,
    val password: String,
)

@Serializable
data class AuthResponse(
    val token: String,
    val username: String,
)

@Serializable
data class HostedErrorBody(
    val error: String = "",
)

/** Reading place and sharing. The server owns postCount now that it parses books. */
@Serializable
data class PatchBook(
    val progressIndex: Int? = null,
    val shared: Boolean? = null,
)

@Serializable
data class PostsResponse(
    val posts: List<FeedPost> = emptyList(),
)

/** Public `GET /v1/version`. Commit is omitted when the server has none. */
@Serializable
data class ServerVersion(
    val version: String = "",
    val commit: String? = null,
)

open class HostedClient(
    private val http: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(30, TimeUnit.SECONDS)
        .readTimeout(120, TimeUnit.SECONDS)
        .writeTimeout(120, TimeUnit.SECONDS)
        .build(),
) {
    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = false
    }

    fun register(baseUrl: String, username: String, password: String): AuthResponse =
        postAuth("$baseUrl/v1/auth/register", username, password)

    fun login(baseUrl: String, username: String, password: String): AuthResponse =
        postAuth("$baseUrl/v1/auth/login", username, password)

    fun logout(session: HostedSession) {
        val url = session.api("/v1/auth/logout")
        http.newCall(
            request(url, session.token).post(ByteArray(0).toRequestBody(null)).build(),
        ).execute().use { it.expectOk() }
    }

    open fun library(session: HostedSession): LibrarySnapshot {
        return get(session, "/v1/library")
    }

    /** No session. The route is public. */
    fun version(baseUrl: String): ServerVersion {
        val base = normalizeBaseUrl(baseUrl).ifBlank {
            throw HostedException("Set a server URL first.")
        }
        val req = Request.Builder().url("$base/v1/version").get().build()
        return http.newCall(req).execute().use { it.decode() }
    }

    fun upload(
        session: HostedSession,
        epub: File,
        cover: File?,
        title: String,
        author: String,
        handle: String,
        isSample: Boolean,
    ): BookRecord {
        val body = MultipartBody.Builder()
            .setType(MultipartBody.FORM)
            .addFormDataPart("title", title)
            .addFormDataPart("author", author)
            .addFormDataPart("handle", handle)
            .addFormDataPart("isSample", isSample.toString())
            .addFormDataPart(
                "epub",
                "book.epub",
                epub.asRequestBody("application/epub+zip".toMediaType()),
            )
            .apply {
                if (cover != null && cover.exists() && cover.length() > 0L) {
                    addFormDataPart(
                        "cover",
                        "cover.jpg",
                        cover.asRequestBody("image/jpeg".toMediaType()),
                    )
                }
            }
            .build()
        val req = request(session.api("/v1/books"), session.token)
            .post(body)
            .build()
        return http.newCall(req).execute().use { it.decode() }
    }

    fun delete(session: HostedSession, id: String) {
        val req = request(session.api("/v1/books/$id"), session.token)
            .delete()
            .build()
        http.newCall(req).execute().use { it.expectOk() }
    }

    fun patch(session: HostedSession, id: String, body: PatchBook): BookRecord {
        val req = request(session.api("/v1/books/$id"), session.token)
            .patch(json.encodeToString(body).toRequestBody(JSON))
            .build()
        return http.newCall(req).execute().use { it.decode() }
    }

    /** Owner-only. Another account that can see the book gets 403. */
    fun setShared(session: HostedSession, id: String, shared: Boolean): BookRecord {
        return patch(session, id, PatchBook(shared = shared))
    }

    /** Timeline posts the server parsed from this book. Ids match likes and comments. */
    open fun posts(session: HostedSession, id: String): List<FeedPost> {
        val body: PostsResponse = get(session, "/v1/books/$id/posts")
        return body.posts
    }

    open fun downloadCover(session: HostedSession, id: String, dest: File): Boolean {
        return try {
            download(session, "/v1/books/$id/cover", dest)
            dest.exists() && dest.length() > 0L
        } catch (err: HostedException) {
            if (err.code == 401) throw err
            false
        }
    }

    fun likes(session: HostedSession, id: String): Set<String> {
        val set: LikeSet = get(session, "/v1/books/$id/likes")
        return set.ids
    }

    fun toggleLike(session: HostedSession, id: String, postId: String): Set<String> {
        val req = request(session.api("/v1/books/$id/likes/$postId"), session.token)
            .post(ByteArray(0).toRequestBody(null))
            .build()
        val set: LikeSet = http.newCall(req).execute().use { it.decode() }
        return set.ids
    }

    fun commentIndex(session: HostedSession, id: String): CommentIndex {
        return get(session, "/v1/books/$id/comments")
    }

    fun comments(session: HostedSession, id: String, postId: String): List<PostComment> {
        val thread: CommentThread = get(session, "/v1/books/$id/comments/$postId")
        return thread.comments
    }

    fun addComment(
        session: HostedSession,
        id: String,
        postId: String,
        text: String,
    ): List<PostComment> {
        val req = request(session.api("/v1/books/$id/comments/$postId"), session.token)
            .post(json.encodeToString(NewComment(text)).toRequestBody(JSON))
            .build()
        val thread: CommentThread = http.newCall(req).execute().use { it.decode() }
        return thread.comments
    }

    fun profileReplies(session: HostedSession, handle: String): ProfileReplies {
        return get(session, "/v1/profiles/${android.net.Uri.encode(handle)}/replies")
    }

    fun search(session: HostedSession, query: String, kind: SearchKind, cursor: String?): SearchPage {
        val q = android.net.Uri.encode(query)
        val type = android.net.Uri.encode(kind.wire)
        val cursorPart = if (cursor.isNullOrEmpty()) "" else "&cursor=${android.net.Uri.encode(cursor)}"
        return get(session, "/v1/search?q=$q&type=$type$cursorPart")
    }

    fun user(session: HostedSession, username: String): ProfileRecord {
        return get(session, "/v1/users/${android.net.Uri.encode(username)}")
    }

    fun userPosts(session: HostedSession, username: String, cursor: String?): ProfilePostsPage {
        return get(session, userPath(username, "posts", cursor))
    }

    fun userReposts(session: HostedSession, username: String, cursor: String?): ProfileRepostsPage {
        return get(session, userPath(username, "reposts", cursor))
    }

    fun userBooks(session: HostedSession, username: String): ProfileBooks {
        return get(session, userPath(username, "books", null))
    }

    fun homeFeed(session: HostedSession, cursor: String?): HomeFeedPage {
        val cursorPart = if (cursor.isNullOrEmpty()) "" else "?cursor=${android.net.Uri.encode(cursor)}"
        return get(session, "/v1/feed$cursorPart")
    }

    fun setRepost(session: HostedSession, id: String, postId: String, reposted: Boolean): RepostState {
        val url = session.api("/v1/books/$id/posts/${android.net.Uri.encode(postId)}/repost")
        val call = request(url, session.token)
            .method(if (reposted) "PUT" else "DELETE", ByteArray(0).toRequestBody(null))
            .build()
        return http.newCall(call).execute().use { it.decode() }
    }

    private fun userPath(username: String, suffix: String, cursor: String?): String {
        val cursorPart = if (cursor.isNullOrEmpty()) "" else "?cursor=${android.net.Uri.encode(cursor)}"
        return "/v1/users/${android.net.Uri.encode(username)}/$suffix$cursorPart"
    }

    fun deleteComment(
        session: HostedSession,
        id: String,
        postId: String,
        commentId: String,
    ): List<PostComment> {
        val req = request(
            session.api("/v1/books/$id/comments/$postId/$commentId"),
            session.token,
        ).delete().build()
        val thread: CommentThread = http.newCall(req).execute().use { it.decode() }
        return thread.comments
    }

    private fun postAuth(url: String, username: String, password: String): AuthResponse {
        val req = Request.Builder()
            .url(url)
            .post(json.encodeToString(AuthRequest(username, password)).toRequestBody(JSON))
            .build()
        return http.newCall(req).execute().use { it.decode() }
    }

    private inline fun <reified T> get(session: HostedSession, path: String): T {
        val req = request(session.api(path), session.token).get().build()
        return http.newCall(req).execute().use { it.decode() }
    }

    private fun download(session: HostedSession, path: String, dest: File) {
        val req = request(session.api(path), session.token).get().build()
        http.newCall(req).execute().use { response ->
            response.expectOk()
            dest.parentFile?.mkdirs()
            val tmp = File(dest.parentFile, dest.name + ".part")
            response.body?.byteStream()?.use { input ->
                tmp.outputStream().use { input.copyTo(it) }
            } ?: throw HostedException("Empty download.")
            if (!tmp.renameTo(dest)) {
                tmp.copyTo(dest, overwrite = true)
                tmp.delete()
            }
        }
    }

    private fun request(url: String, token: String?): Request.Builder {
        val builder = Request.Builder().url(url)
        if (!token.isNullOrBlank()) {
            builder.header("Authorization", "Bearer $token")
        }
        return builder
    }

    private fun HostedSession.api(path: String): String {
        val base = normalizedUrl() ?: throw HostedException("Set a server URL first.")
        return base + path
    }

    private inline fun <reified T> Response.decode(): T {
        expectOk()
        val text = body?.string().orEmpty()
        return runCatching { json.decodeFromString<T>(text) }.getOrElse {
            throw HostedException("Could not read the server response.", code)
        }
    }

    private fun Response.expectOk() {
        if (isSuccessful) return
        val text = try {
            body?.string().orEmpty()
        } catch (_: IOException) {
            ""
        }
        val message = runCatching { json.decodeFromString<HostedErrorBody>(text).error }
            .getOrNull()
            ?.takeIf { it.isNotBlank() }
            ?: "Server returned $code"
        throw HostedException(message, code)
    }

    companion object {
        private val JSON = "application/json; charset=utf-8".toMediaType()
    }
}
