package com.mewo.reader.data

import kotlinx.serialization.json.Json
import java.io.File
import java.security.MessageDigest

/** One shelf file per server URL and username. */
fun shelfCacheKey(serverUrl: String, username: String): String {
    val material = normalizeBaseUrl(serverUrl) + "\n" + username
    val digest = MessageDigest.getInstance("SHA-256").digest(material.toByteArray(Charsets.UTF_8))
    return digest.joinToString("") { byte -> "%02x".format(byte) }
}

class ShelfCache(
    private val dir: File,
    private val json: Json,
) {
    fun read(serverUrl: String, username: String): List<BookRecord>? {
        if (serverUrl.isBlank() || username.isBlank()) return null
        val file = file(serverUrl, username)
        if (!file.exists()) return null
        return runCatching {
            json.decodeFromString<LibrarySnapshot>(file.readText()).books
        }.getOrNull()
    }

    fun write(serverUrl: String, username: String, books: List<BookRecord>) {
        if (serverUrl.isBlank() || username.isBlank()) return
        dir.mkdirs()
        val dest = file(serverUrl, username)
        val tmp = File(dir, dest.name + ".tmp")
        tmp.writeText(json.encodeToString(LibrarySnapshot(books)))
        if (!tmp.renameTo(dest)) {
            tmp.copyTo(dest, overwrite = true)
            tmp.delete()
        }
    }

    fun clear(serverUrl: String, username: String) {
        if (serverUrl.isBlank() || username.isBlank()) return
        file(serverUrl, username).delete()
    }

    private fun file(serverUrl: String, username: String): File {
        return File(dir, shelfCacheKey(serverUrl, username) + ".json")
    }
}
