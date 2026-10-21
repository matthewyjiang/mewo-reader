package com.mewo.reader.data

import kotlinx.serialization.Serializable

/** `GET /v1/me`. `canRequest` is the master switch for the catalog. */
@Serializable
data class Me(
    val username: String = "",
    val isAdmin: Boolean = false,
    val canRequest: Boolean = false,
)

@Serializable
data class BookRequest(
    val id: String,
    val workKey: String,
    val title: String,
    val author: String = "",
    val year: Int? = null,
    val coverId: Int? = null,
    val state: String,
    val progress: Int = 0,
    val error: String? = null,
    val bookId: String? = null,
    val requester: String = "",
    val createdAt: Long = 0,
    val updatedAt: Long = 0,
    /** Leader username. Present only on the admin list. */
    val follows: String? = null,
)

/** A copy of a catalog work this account can open. */
@Serializable
data class ServerCopy(
    val bookId: String,
    /** Empty when the viewer holds a private copy. */
    val ownerUsername: String = "",
    val mine: Boolean = false,
    val shared: Boolean = false,
    val held: Boolean = false,
)

@Serializable
data class CatalogWork(
    val workKey: String,
    val title: String,
    val author: String = "",
    val year: Int? = null,
    val coverId: Int? = null,
    val request: BookRequest? = null,
    /** Null when this account cannot open a copy. Older payloads omit it. */
    val onServer: ServerCopy? = null,
)

@Serializable
data class Rating(
    val average: Double,
    val count: Int = 0,
)

@Serializable
data class WorkInfo(
    val canonicalKey: String,
    val title: String,
    val subtitle: String? = null,
    val authors: List<String> = emptyList(),
    val year: Int? = null,
    val coverId: Int? = null,
    val description: String? = null,
    val subjects: List<String> = emptyList(),
    val awards: List<String> = emptyList(),
    val series: List<String> = emptyList(),
    val editionCount: Int? = null,
    val pagesMedian: Int? = null,
    val languages: List<String> = emptyList(),
    val hasAcceptedEdition: Boolean = false,
    val rating: Rating? = null,
)

/**
 * `GET /v1/catalog/works/{olid}` flattens [BookRequest] next to these fields.
 * `phase` is the server's sheet label.
 */
@Serializable
data class RequestDetail(
    val id: String,
    val workKey: String,
    val title: String,
    val author: String = "",
    val year: Int? = null,
    val coverId: Int? = null,
    val state: String,
    val progress: Int = 0,
    val error: String? = null,
    val bookId: String? = null,
    val requester: String = "",
    val createdAt: Long = 0,
    val updatedAt: Long = 0,
    val follows: String? = null,
    val attempts: Int = 0,
    val maxAttempts: Int = 0,
    val nextCheckAt: Long? = null,
    val releaseTitle: String? = null,
    val releaseSize: Long? = null,
    val phase: String,
) {
    fun asBookRequest(): BookRequest = BookRequest(
        id = id,
        workKey = workKey,
        title = title,
        author = author,
        year = year,
        coverId = coverId,
        state = state,
        progress = progress,
        error = error,
        bookId = bookId,
        requester = requester,
        createdAt = createdAt,
        updatedAt = updatedAt,
        follows = follows,
    )
}

@Serializable
data class WorkStatus(
    val request: RequestDetail? = null,
    val onServer: ServerCopy? = null,
    val active: Int = 0,
    val cap: Int = 0,
    val otherRequests: List<BookRequest> = emptyList(),
)

@Serializable
data class WorkDetail(
    val workKey: String,
    val info: WorkInfo? = null,
    val infoError: String? = null,
    val status: WorkStatus = WorkStatus(),
)

@Serializable
data class CatalogPage(
    val items: List<CatalogWork> = emptyList(),
    val nextPage: Int? = null,
)

@Serializable
data class RequestBookBody(
    val workKey: String,
    val title: String,
    val author: String,
    val year: Int? = null,
    val coverId: Int? = null,
)

/** Sheet label. [live] is still moving and worth polling. */
enum class RequestPhase(val wire: String) {
    Searching("searching"),
    Waiting("waiting"),
    NoMatch("no_match"),
    Held("held"),
    Retrying("retrying"),
    Downloading("downloading"),
    Importing("importing"),
    Failed("failed"),
    Canceled("canceled"),
    Imported("imported"),
    ;

    val live: Boolean
        get() = this == Searching || this == Waiting || this == NoMatch ||
            this == Held || this == Retrying || this == Downloading || this == Importing

    companion object {
        fun fromWire(value: String?): RequestPhase? = entries.find { it.wire == value }
    }
}

enum class CatalogAccess {
    Unknown,
    Off,
    On,
}

/**
 * Catalog failure whose [message] is already something a person can read.
 * Callers show it as-is.
 */
class CatalogException(
    val reason: Reason,
    override val message: String,
) : Exception(message) {
    enum class Reason {
        Off,
        Unauthorized,
        NotFound,
        Network,
        Failed,
    }
}

/** `OL123W` from `/works/OL123W`. */
fun catalogOlid(workKey: String): String {
    val slash = workKey.lastIndexOf('/')
    return if (slash >= 0) workKey.substring(slash + 1) else workKey
}
