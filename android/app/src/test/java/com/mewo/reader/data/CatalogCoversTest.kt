package com.mewo.reader.data

import kotlinx.coroutines.test.runTest
import okhttp3.OkHttpClient
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.IOException
import java.nio.file.Files

class CatalogCoversTest {
    @Test
    fun aNetworkFailureIsAMissingCover() = runTest {
        val client = OkHttpClient.Builder()
            .addInterceptor { throw IOException("offline") }
            .build()
        val covers = CatalogCovers(Files.createTempDirectory("catalog-covers").toFile(), client)
        assertNull(covers.load(123))
    }
}
