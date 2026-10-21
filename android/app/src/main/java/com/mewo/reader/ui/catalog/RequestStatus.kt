package com.mewo.reader.ui.catalog

import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.data.RequestDetail
import com.mewo.reader.data.RequestPhase
import com.mewo.reader.data.ServerCopy
import com.mewo.reader.data.WorkDetail
import com.mewo.reader.data.WorkInfo
import com.mewo.reader.data.WorkStatus
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/** Fewer than this many Unicode scalars is trending, not a search. */
const val CATALOG_MIN_QUERY = 3

const val CATALOG_DEBOUNCE_MS = 300L

const val CATALOG_POLL_MS = 10_000L

private val HOLD_SERVICES = listOf("SABnzbd", "Prowlarr", "Open Library")

sealed class CatalogQuery {
    data object Browse : CatalogQuery()
    data class Search(val q: String) : CatalogQuery()
}

/** Blank, or fewer than [CATALOG_MIN_QUERY] Unicode scalars (an emoji counts as one), stays on trending. */
fun catalogQuery(raw: String): CatalogQuery {
    val trimmed = raw.trim()
    val scalars = trimmed.codePointCount(0, trimmed.length)
    return if (scalars < CATALOG_MIN_QUERY) CatalogQuery.Browse else CatalogQuery.Search(trimmed)
}

/** Service name from a held-upstream error, such as `SABnzbd: down`. */
fun holdService(error: String?): String? {
    if (error.isNullOrEmpty()) return null
    return HOLD_SERVICES.firstOrNull { error.startsWith("$it:") }
}

/** Title inside `No matching release (closest: …, a different book)`. */
fun closestTitle(error: String?): String? {
    if (error.isNullOrEmpty()) return null
    val match = Regex("\\(closest: (.*?), a different book\\)").find(error) ?: return null
    return match.groupValues[1]
}

/** Same rule as the server's `phase`, for a list row that has no `phase` yet. */
fun requestPhase(request: BookRequest): RequestPhase = requestPhase(
    state = request.state,
    progress = request.progress,
    error = request.error,
    phase = null,
)

fun requestPhase(request: RequestDetail): RequestPhase = requestPhase(
    state = request.state,
    progress = request.progress,
    error = request.error,
    phase = request.phase,
)

fun requestPhase(state: String, progress: Int, error: String?, phase: String?): RequestPhase {
    RequestPhase.fromWire(phase)?.let { return it }
    val text = error.orEmpty()
    if (state == "imported") return RequestPhase.Imported
    if (state == "failed") return RequestPhase.Failed
    if (state == "canceled") return RequestPhase.Canceled
    if (state == "downloading") {
        if (holdService(text) != null) return RequestPhase.Held
        if (progress >= 100) return RequestPhase.Importing
        return RequestPhase.Downloading
    }
    if (state == "requested") {
        if (text == "No release yet.") return RequestPhase.Waiting
        if (text.startsWith("No matching release")) return RequestPhase.NoMatch
        if (holdService(text) != null) return RequestPhase.Held
        if (text.isNotEmpty()) return RequestPhase.Retrying
    }
    return RequestPhase.Searching
}

/** Searching, waiting, downloading, on the shelf, failed, or canceled.
 *  A server `phase` wins. Without one, the list wire keeps the older labels. */
fun stateLabel(request: BookRequest): String = legacyStateLabel(
    request.state,
    request.progress,
    request.error,
)

fun stateLabel(request: RequestDetail): String {
    RequestPhase.fromWire(request.phase)?.let { phase ->
        return phaseLabel(phase, request.progress, request.error)
    }
    return legacyStateLabel(request.state, request.progress, request.error)
}

private fun legacyStateLabel(state: String, progress: Int, error: String?): String = when (state) {
    "requested" -> if (error == "No release yet.") "Waiting for a release" else "Searching"
    "downloading" -> "Downloading $progress%"
    "imported" -> "On your shelf"
    "failed" -> "Failed: ${error.orEmpty()}"
    else -> "Canceled"
}

private fun phaseLabel(phase: RequestPhase, progress: Int, error: String?): String = when (phase) {
    RequestPhase.Searching -> "Searching"
    RequestPhase.Waiting -> "Waiting for a release"
    RequestPhase.NoMatch -> "No matching release"
    RequestPhase.Held -> {
        val service = holdService(error)
        if (service != null) "Waiting on $service" else "Waiting"
    }
    RequestPhase.Retrying -> "Retrying"
    RequestPhase.Downloading -> "Downloading $progress%"
    RequestPhase.Importing -> "Importing"
    RequestPhase.Imported -> "On your shelf"
    RequestPhase.Failed -> "Failed: ${error.orEmpty()}"
    RequestPhase.Canceled -> "Canceled"
}

enum class RowAction { Request, Pending, Open, Retry }

/** The catalog row's trailing action. A copy the viewer can open wins over Request,
 *  except while their own request is still running or failed. */
fun rowAction(request: BookRequest?, onServer: ServerCopy?): RowAction {
    if (request != null && (request.state == "requested" || request.state == "downloading")) {
        return RowAction.Pending
    }
    if (request?.state == "failed") return RowAction.Retry
    if (request?.state == "imported" && request.bookId != null) return RowAction.Open
    if (onServer?.bookId != null) return RowAction.Open
    if (request != null && (request.state == "canceled" || request.state == "imported")) {
        return RowAction.Retry
    }
    return RowAction.Request
}

/** Held and owned copies say "On your shelf". Someone else's shared copy says "On server". */
fun openLabel(work: CatalogWork, request: BookRequest?): String {
    if (request?.state == "imported" || request?.bookId != null) return "On your shelf"
    val copy = work.onServer
    if (copy == null || copy.mine || copy.held) return "On your shelf"
    return "On server"
}

enum class CopyStyle { Badge, Sheet }

/** A held copy never names its owner, even when [ServerCopy.ownerUsername] is filled in. */
fun copyLabel(copy: ServerCopy, style: CopyStyle): String {
    if (copy.mine || copy.held) return "On your shelf"
    if (copy.shared && copy.ownerUsername.isNotEmpty()) {
        return if (style == CopyStyle.Badge) {
            "On server · @${copy.ownerUsername}"
        } else {
            "On server, shared by @${copy.ownerUsername}"
        }
    }
    return "On server"
}

/** Own active or failed request, then a copy on the server, then nothing. */
fun badge(work: CatalogWork, request: BookRequest?): String? {
    if (request != null &&
        (request.state == "requested" || request.state == "downloading" || request.state == "failed")
    ) {
        return stateLabel(request)
    }
    if (request?.state == "imported" && request.bookId != null) return "On your shelf"
    val copy = work.onServer
    if (copy != null) return copyLabel(copy, CopyStyle.Badge)
    return null
}

enum class BadgeTone { Shelf, Failed, Server, Progress }

fun badgeTone(label: String): BadgeTone {
    if (label.startsWith("Failed")) return BadgeTone.Failed
    if (label == "On your shelf") return BadgeTone.Shelf
    if (label.startsWith("On server")) return BadgeTone.Server
    return BadgeTone.Progress
}

fun catalogRowLabel(work: CatalogWork, request: BookRequest?): String {
    val parts = mutableListOf(work.title)
    if (work.author.isNotEmpty()) parts += work.author
    if (work.year != null) parts += work.year.toString()
    badge(work, request)?.let { parts += it }
    return parts.joinToString(", ")
}

/** Viewer's locale and time zone. The sheet says when the next search runs. */
fun formatCheck(at: Long): String {
    val zoned = Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault())
    return DateTimeFormatter.ofPattern("MMM d, h:mm a", Locale.getDefault()).format(zoned)
}

fun waitingCopy(nextCheckAt: Long?): String {
    if (nextCheckAt == null) return "No release yet."
    return "No release yet. Next search ${formatCheck(nextCheckAt)}"
}

fun noMatchCopy(error: String?, nextCheckAt: Long?): String {
    val closest = closestTitle(error)
    val found = if (closest != null) {
        "Only found a different book ($closest)."
    } else {
        "Only found a different book."
    }
    if (nextCheckAt == null) return found
    return "$found Searching again ${formatCheck(nextCheckAt)}."
}

fun heldCopy(error: String?): String {
    val service = holdService(error)
    if (service == null) {
        return if (!error.isNullOrEmpty()) "Waiting: $error" else "Waiting on the download service."
    }
    val rest = error?.drop(service.length + 1)?.trim().orEmpty()
    return if (rest.isNotEmpty()) "Waiting on $service: $rest" else "Waiting on $service"
}

/** One muted line. The first three names, then +N for the rest. */
fun alsoRequested(names: List<String>): String = formatAlsoRequested(names)

private fun formatAlsoRequested(names: List<String>): String {
    val shown = names.take(3).map { "@$it" }
    val extra = names.size - shown.size
    val list = shown.joinToString(", ")
    return if (extra > 0) "Also requested by $list +$extra" else "Also requested by $list"
}

/** Year, length, and average. Edition count and rating count stay off this line. */
fun workStats(info: WorkInfo): String {
    val parts = mutableListOf<String>()
    info.year?.let { parts += "First published $it" }
    info.pagesMedian?.let { pages ->
        parts += "~${String.format(Locale.getDefault(), "%,d", pages)} pages"
    }
    info.rating?.let { rating ->
        // Half up, the same way `Number#toFixed(1)` prints 4.25 as 4.3.
        parts += "★ ${String.format(Locale.US, "%.1f", rating.average)}"
    }
    return parts.joinToString(" · ")
}

/** Plain text. Subjects are not links, so they are not chips. */
fun subjectLine(subjects: List<String>): String = subjects.take(5).joinToString(" · ")

/** Open Library cover CDN, medium JPEG. `default=false` makes a missing cover
 *  404 instead of a 1×1 GIF. Not proxied. */
fun coverUrl(coverId: Int?): String? {
    if (coverId == null || coverId < 0) return null
    return "https://covers.openlibrary.org/b/id/$coverId-M.jpg?default=false"
}

sealed class SheetStatus {
    data class Live(val phase: RequestPhase, val line: String, val progress: Int? = null) : SheetStatus()
    data class Failed(val line: String, val capNote: String? = null) : SheetStatus()
    data class Available(val line: String, val bookId: String? = null) : SheetStatus()
    data class Canceled(val capNote: String? = null) : SheetStatus()
    data class Requestable(val capNote: String? = null, val editionNote: String? = null) : SheetStatus()
}

fun capNote(status: WorkStatus?): String? {
    if (status == null) return null
    if (status.active >= status.cap) return "You have ${status.cap} requests in progress"
    return null
}

fun sheetStatus(work: CatalogWork, detail: WorkDetail?, fallback: BookRequest?): SheetStatus {
    val detailRequest = detail?.status?.request
    val request = detailRequest?.asBookRequest() ?: fallback
    val phase = when {
        detailRequest != null -> requestPhase(detailRequest)
        request != null -> requestPhase(request)
        else -> null
    }
    val copy = detail?.status?.onServer ?: work.onServer
    val note = capNote(detail?.status)
    val bookId = detailRequest?.bookId
        ?: detail?.status?.onServer?.bookId
        ?: fallback?.bookId
        ?: work.onServer?.bookId
    if (phase != null && phase.live) {
        val (line, progress) = liveLine(phase, request, detailRequest)
        return SheetStatus.Live(phase, line, progress)
    }
    if (phase == RequestPhase.Failed && request != null) {
        return SheetStatus.Failed(request.error ?: "The download failed.", note)
    }
    if (copy != null || phase == RequestPhase.Imported) {
        val line = if (copy != null) copyLabel(copy, CopyStyle.Sheet) else "On your shelf"
        return SheetStatus.Available(line, bookId)
    }
    if (phase == RequestPhase.Canceled) return SheetStatus.Canceled(note)
    val edition = detail?.info?.let { info ->
        if (!info.hasAcceptedEdition) "No English edition listed on Open Library" else null
    }
    return SheetStatus.Requestable(note, edition)
}

private fun liveLine(
    phase: RequestPhase,
    request: BookRequest?,
    detail: RequestDetail?,
): Pair<String, Int?> = when (phase) {
    RequestPhase.Searching -> "Looking for a release…" to null
    RequestPhase.Waiting -> waitingCopy(detail?.nextCheckAt) to null
    RequestPhase.NoMatch -> noMatchCopy(request?.error, detail?.nextCheckAt) to null
    RequestPhase.Held -> heldCopy(request?.error) to null
    RequestPhase.Downloading -> {
        val pct = request?.progress ?: 0
        "Downloading $pct%" to pct
    }
    RequestPhase.Retrying -> {
        val text = request?.error?.takeIf { it.isNotEmpty() } ?: "Trying another release…"
        text to null
    }
    RequestPhase.Importing -> "Checking and importing…" to (request?.progress ?: 0)
    else -> "Checking and importing…" to (request?.progress ?: 0)
}

/**
 * Words for a catalog failure. A raw status line (`HTTP 500`, `Server returned 500`)
 * is replaced. A sentence the server wrote is kept.
 */
fun catalogErrorText(code: Int, serverMessage: String?, network: Boolean): String {
    if (network) return "Could not reach the server. Check your connection and try again."
    val raw = serverMessage?.trim().orEmpty()
    if (raw.isNotEmpty() && !rawStatus(raw)) return raw
    return when (code) {
        401 -> "Sign in again."
        403 -> "Requests are off."
        404 -> "That book is no longer in the catalog."
        else -> "Could not load the catalog. Try again."
    }
}

private fun rawStatus(text: String): Boolean {
    return Regex("""(?i)^(server returned \d+|http\s*\d+|http/\d).*""").matches(text)
}

data class CatalogAbout(
    val description: String?,
    val series: List<String>,
    val awards: List<String>,
    val subjects: String?,
    val stats: String?,
    val openLibraryUrl: String,
)
