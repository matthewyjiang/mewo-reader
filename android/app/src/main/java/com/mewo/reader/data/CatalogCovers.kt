package com.mewo.reader.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * Open Library covers, saved under cacheDir/catalog-covers. Not proxied.
 * A 404 is a missing cover, not an error. No Coil.
 */
class CatalogCovers(
    cacheDir: File,
    private val http: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build(),
) {
    private val root = File(cacheDir, "catalog-covers").apply { mkdirs() }

    fun cached(coverId: Int): File? {
        val file = File(root, "$coverId.jpg")
        return file.takeIf { it.exists() && it.length() > 0L }
    }

    suspend fun load(coverId: Int?): File? = withContext(Dispatchers.IO) {
        if (coverId == null || coverId < 0) return@withContext null
        cached(coverId)?.let { return@withContext it }
        val url = "https://covers.openlibrary.org/b/id/$coverId-M.jpg?default=false"
        val req = Request.Builder().url(url).get().build()
        http.newCall(req).execute().use { response ->
            if (response.code == 404) return@withContext null
            if (!response.isSuccessful) return@withContext null
            val bytes = response.body?.bytes() ?: return@withContext null
            if (bytes.isEmpty()) return@withContext null
            val dest = File(root, "$coverId.jpg")
            val tmp = File(root, "$coverId.jpg.part")
            tmp.writeBytes(bytes)
            if (!tmp.renameTo(dest)) {
                tmp.copyTo(dest, overwrite = true)
                tmp.delete()
            }
            dest.takeIf { it.exists() && it.length() > 0L }
        }
    }
}
