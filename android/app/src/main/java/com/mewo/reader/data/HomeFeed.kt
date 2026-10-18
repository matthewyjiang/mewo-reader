package com.mewo.reader.data

import kotlinx.serialization.json.Json
import java.io.File
import java.security.MessageDigest

/** Not loaded yet. Empty copy is not this state. */
sealed class FeedPhase {
    data object Loading : FeedPhase()

    data class Loaded(
        val page: HomeFeedPage,
        val refreshing: Boolean,
        val more: Boolean,
    ) : FeedPhase()

    data class Failed(
        val error: String,
        val page: HomeFeedPage?,
    ) : FeedPhase()
}

fun showFeedEmpty(phase: FeedPhase): Boolean {
    val page = when (phase) {
        FeedPhase.Loading -> return false
        is FeedPhase.Loaded -> phase.page
        is FeedPhase.Failed -> phase.page ?: return false
    }
    return page.items.isEmpty()
}

fun beginFeedRefresh(phase: FeedPhase): FeedPhase = when (phase) {
    is FeedPhase.Loaded -> phase.copy(refreshing = true, more = false)
    is FeedPhase.Failed -> phase.page?.let {
        FeedPhase.Loaded(it, refreshing = true, more = false)
    } ?: FeedPhase.Loading
    FeedPhase.Loading -> FeedPhase.Loading
}

fun finishFeedRefresh(page: HomeFeedPage): FeedPhase =
    FeedPhase.Loaded(page, refreshing = false, more = false)

fun failFeed(phase: FeedPhase, error: String): FeedPhase {
    val page = when (phase) {
        FeedPhase.Loading -> null
        is FeedPhase.Loaded -> phase.page
        is FeedPhase.Failed -> phase.page
    }
    return FeedPhase.Failed(error, page)
}

fun phaseFromFeedCache(page: HomeFeedPage?): FeedPhase =
    if (page == null) FeedPhase.Loading else FeedPhase.Loaded(page, refreshing = true, more = false)

/** The last successful first page, keyed by server URL and username. */
interface HomeFeedCache {
    fun read(session: String): HomeFeedPage?
    fun write(session: String, page: HomeFeedPage)
    fun clear(session: String)
}

class MemoryFeedCache : HomeFeedCache {
    private val pages = mutableMapOf<String, HomeFeedPage>()
    override fun read(session: String): HomeFeedPage? = pages[session]
    override fun write(session: String, page: HomeFeedPage) { pages[session] = page }
    override fun clear(session: String) { pages.remove(session) }
}

/** One file per session key. The key is the server URL and username. */
class FileHomeFeedCache(
    private val dir: File,
    private val json: Json = Json { ignoreUnknownKeys = true },
) : HomeFeedCache {
    override fun read(session: String): HomeFeedPage? {
        if (session.isBlank()) return null
        val file = file(session)
        if (!file.exists()) return null
        return runCatching { json.decodeFromString<HomeFeedPage>(file.readText()) }.getOrNull()
    }

    override fun write(session: String, page: HomeFeedPage) {
        if (session.isBlank()) return
        dir.mkdirs()
        val dest = file(session)
        val tmp = File(dir, dest.name + ".tmp")
        tmp.writeText(json.encodeToString(page))
        if (!tmp.renameTo(dest)) {
            tmp.copyTo(dest, overwrite = true)
            tmp.delete()
        }
    }

    override fun clear(session: String) {
        if (session.isBlank()) return
        file(session).delete()
    }

    private fun file(session: String): File {
        val digest = MessageDigest.getInstance("SHA-256").digest(session.toByteArray(Charsets.UTF_8))
        val name = digest.joinToString("") { byte -> "%02x".format(byte) }
        return File(dir, "$name.json")
    }
}
