package com.mewo.reader.ui.search

import com.mewo.reader.data.SearchKind
import com.mewo.reader.data.SearchPage
import com.mewo.reader.data.isEmpty
import com.mewo.reader.data.kind

const val SEARCH_DEBOUNCE_MS = 250L

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

/** True only after a successful page for this query and tab came back empty. */
fun showNoMatches(state: SearchState): Boolean {
    if (state.query.isBlank()) return false
    val phase = state.phase as? SearchPhase.Loaded ?: return false
    if (phase.more) return false
    return phase.page.kind() == state.kind && phase.page.isEmpty()
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
