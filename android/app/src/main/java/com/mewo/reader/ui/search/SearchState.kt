package com.mewo.reader.ui.search

import com.mewo.reader.data.SearchKind
import com.mewo.reader.data.SearchPage
import com.mewo.reader.data.isEmpty
import com.mewo.reader.data.kind
import com.mewo.reader.ui.catalog.catalogErrorText
import java.io.IOException

const val SEARCH_DEBOUNCE_MS = 250L

/** A browse page older than this is re-fetched quietly. Typed pages are not. */
const val BROWSE_REVALIDATE_MS = 60_000L

/** Accounts, books, and posts. Catalog joins them only when requests are on. */
fun searchTabs(hosted: Boolean, canRequest: Boolean): List<Pair<String, String>> {
    val tabs = if (hosted) {
        listOf(
            "accounts" to "Accounts",
            "books" to "Books",
            "posts" to "Posts",
        )
    } else {
        listOf(
            "books" to "Books",
            "posts" to "Posts",
        )
    }
    return if (canRequest) tabs + ("catalog" to "Catalog") else tabs
}

sealed class SearchPhase {
    data object Idle : SearchPhase()
    data class Loading(val previous: SearchPage? = null) : SearchPhase()
    data class Loaded(val page: SearchPage, val more: Boolean) : SearchPhase()
    data class Failed(val error: String, val previous: SearchPage? = null) : SearchPhase()
}

data class SearchState(
    val query: String = "",
    val kind: SearchKind = SearchKind.Accounts,
    val phase: SearchPhase = SearchPhase.Idle,
    val session: String? = null,
)

fun visiblePage(state: SearchState): SearchPage? = when (val phase = state.phase) {
    SearchPhase.Idle -> null
    is SearchPhase.Loading -> phase.previous
    is SearchPhase.Loaded -> phase.page
    is SearchPhase.Failed -> phase.previous
}

enum class EmptyKind { None, NoMatches, NothingYet }

/** Blank text is browse. Catalog keeps its own rule in catalogQuery(). */
fun isBrowse(state: SearchState): Boolean = state.query.isBlank()

/** A finished empty page. Browse and a typed miss use different copy. Idle (local blank) is None. */
fun emptyKind(state: SearchState): EmptyKind {
    val phase = state.phase as? SearchPhase.Loaded ?: return EmptyKind.None
    if (phase.more || phase.page.kind() != state.kind || !phase.page.isEmpty()) return EmptyKind.None
    return if (isBrowse(state)) EmptyKind.NothingYet else EmptyKind.NoMatches
}

/** Single-title headings, same strings as web `sectionFor`. Catalog's heading stays in CatalogResults. */
fun sectionTitle(kind: SearchKind): String = when (kind) {
    SearchKind.Accounts -> "Suggested accounts"
    SearchKind.Books -> "Popular on this server"
    SearchKind.Posts -> "Top posts"
    SearchKind.Catalog -> "Trending on Open Library"
}

/** Empty copy. Same words as web `SearchEmpty.svelte`. */
fun emptyCopy(kind: SearchKind, empty: EmptyKind, query: String): String {
    val quoted = "“${query.trim()}”"
    return when (empty) {
        EmptyKind.None -> ""
        EmptyKind.NothingYet -> when (kind) {
            SearchKind.Accounts -> "It's just you here so far."
            SearchKind.Books -> "No books yet."
            SearchKind.Posts -> "No posts yet. Replies and reposts on books show up here."
            SearchKind.Catalog -> "No trending books in your languages right now. Search above."
        }
        EmptyKind.NoMatches -> when (kind) {
            SearchKind.Accounts -> "No accounts match $quoted"
            SearchKind.Books -> "No books match $quoted"
            SearchKind.Posts -> "No posts match $quoted"
            SearchKind.Catalog ->
                "Nothing in Open Library matches $quoted. Try the author's name or fewer words."
        }
    }
}

/** Network failures read like the Catalog's. Everything else keeps its own message. */
fun searchErrorText(error: Exception): String =
    if (error is IOException) catalogErrorText(0, null, network = true)
    else error.message?.takeIf { it.isNotBlank() } ?: "Could not search."

/** One browse page per tab for the signed-in session. */
internal class BrowseMemo(private val now: () -> Long) {
    private data class Entry(val page: SearchPage, val at: Long)

    private val pages = mutableMapOf<SearchKind, Entry>()

    fun recall(kind: SearchKind): SearchPage? = pages[kind]?.page

    /** True once the remembered page is older than [BROWSE_REVALIDATE_MS]. */
    fun stale(kind: SearchKind): Boolean {
        val at = pages[kind]?.at ?: return false
        return now() - at > BROWSE_REVALIDATE_MS
    }

    fun remember(kind: SearchKind, page: SearchPage) {
        pages[kind] = Entry(page, now())
    }

    fun clear() {
        pages.clear()
    }
}

interface SearchKindMemory {
    fun read(): SearchKind?
    fun write(kind: SearchKind)
}

class MemorySearchKind : SearchKindMemory {
    private var kind: SearchKind? = null
    override fun read(): SearchKind? = kind
    override fun write(kind: SearchKind) { this.kind = kind }
}

class PrefSearchKind(context: android.content.Context) : SearchKindMemory {
    private val prefs = context.getSharedPreferences(PREFS, android.content.Context.MODE_PRIVATE)

    override fun read(): SearchKind? = when (prefs.getString(KEY, null)) {
        "accounts" -> SearchKind.Accounts
        "books" -> SearchKind.Books
        "posts" -> SearchKind.Posts
        "catalog" -> SearchKind.Catalog
        else -> null
    }

    override fun write(kind: SearchKind) {
        prefs.edit().putString(KEY, kind.wire).apply()
    }

    private companion object {
        const val PREFS = "mewo_search"
        const val KEY = "kind"
    }
}
