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

class HostedException(message: String) : Exception(message)

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

@Serializable
data class PatchBook(
    val postCount: Int? = null,
    val progressIndex: Int? = null,
)

class HostedClient(
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

    fun library(session: HostedSession): LibrarySnapshot {
        return get(session, "/v1/library")
    }

    fun book(session: HostedSession, id: String): BookRecord {
        return get(session, "/v1/books/$id")
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

    fun downloadEpub(session: HostedSession, id: String, dest: File) {
        download(session, "/v1/books/$id/epub", dest)
    }

    fun downloadCover(session: HostedSession, id: String, dest: File): Boolean {
        return try {
            download(session, "/v1/books/$id/cover", dest)
            dest.exists() && dest.length() > 0L
        } catch (_: HostedException) {
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
            throw HostedException("Could not read the server response.")
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
        throw HostedException(message)
    }

    companion object {
        private val JSON = "application/json; charset=utf-8".toMediaType()
    }
}
