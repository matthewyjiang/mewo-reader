package com.mewo.reader.data

import android.content.Context
import android.graphics.Bitmap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.readium.r2.shared.publication.Publication
import org.readium.r2.shared.publication.services.cover
import org.readium.r2.shared.util.AbsoluteUrl
import org.readium.r2.shared.util.getOrElse
import org.readium.r2.shared.util.http.DefaultHttpClient
import org.readium.r2.streamer.PublicationOpener
import org.readium.r2.streamer.parser.DefaultPublicationParser
import org.readium.r2.shared.util.asset.AssetRetriever
import java.io.File

/**
 * Opens a local EPUB with Readium. PDF support is left out on purpose.
 */
class EpubOpener(context: Context) {
    private val appContext = context.applicationContext
    private val httpClient = DefaultHttpClient()
    private val assetRetriever = AssetRetriever(
        contentResolver = appContext.contentResolver,
        httpClient = httpClient,
    )
    private val publicationOpener = PublicationOpener(
        publicationParser = DefaultPublicationParser(
            context = appContext,
            httpClient = httpClient,
            assetRetriever = assetRetriever,
            pdfFactory = null,
        ),
    )

    suspend fun <T> open(file: File, use: suspend (Publication) -> T): T = withContext(Dispatchers.IO) {
        val url = AbsoluteUrl(file.toURI().toString())
            ?: error("Could not turn ${file.path} into a URL")
        val asset = assetRetriever.retrieve(url).getOrElse { failure ->
            error("Could not read EPUB: $failure")
        }
        val publication = publicationOpener.open(
            asset = asset,
            allowUserInteraction = false,
        ).getOrElse { failure ->
            error("Could not open EPUB: $failure")
        }
        try {
            use(publication)
        } finally {
            publication.close()
        }
    }

    suspend fun metadata(file: File): OpenedMeta = open(file) { publication ->
        val title = publication.metadata.title?.ifBlank { null } ?: file.nameWithoutExtension
        val author = publication.metadata.authors
            .mapNotNull { it.name.ifBlank { null } }
            .firstOrNull()
            ?: "Unknown"
        val cover = publication.cover()
        OpenedMeta(title = title, author = author, cover = cover)
    }
}

data class OpenedMeta(
    val title: String,
    val author: String,
    val cover: Bitmap?,
)
