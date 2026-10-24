package com.mewo.reader.data

sealed class LibraryLoad

/** The shelf has not finished a fetch, and there is nothing saved to show. */
data object LibraryLoading : LibraryLoad()

/**
 * [books] came from a successful fetch. [refreshing] is a background reload
 * of that same list.
 */
data class LibraryLoaded(
    val books: List<BookRecord>,
    val refreshing: Boolean,
) : LibraryLoad()

/**
 * The last fetch failed. [books] is the previous success, or null when this
 * account has never loaded.
 */
data class LibraryFailed(
    val message: String,
    val books: List<BookRecord>?,
) : LibraryLoad()

/** Saved rows, if a fetch has succeeded at least once. */
fun LibraryLoad.cachedBooks(): List<BookRecord>? = when (this) {
    is LibraryLoading -> null
    is LibraryLoaded -> books
    is LibraryFailed -> books
}

fun LibraryLoad.books(): List<BookRecord> = cachedBooks().orEmpty()

/** Empty copy is only for a finished fetch that returned nothing. */
fun LibraryLoad.showEmpty(): Boolean = cachedBooks()?.isEmpty() == true

fun LibraryLoad.isRefreshing(): Boolean = this is LibraryLoaded && refreshing

sealed class LibraryEvent {
    data class Cached(val books: List<BookRecord>) : LibraryEvent()
    data object RefreshStarted : LibraryEvent()
    data class RefreshSucceeded(val books: List<BookRecord>) : LibraryEvent()
    data class RefreshFailed(val message: String) : LibraryEvent()
    data class BooksReplaced(val books: List<BookRecord>) : LibraryEvent()
    data object Dismissed : LibraryEvent()
}

/**
 * Saved text may open a book only while the shelf is still catching up, or when a
 * finished shelf still lists it. A completed refresh is the list.
 */
fun savedTextAllowed(state: LibraryLoad, bookId: String): Boolean = when (state) {
    is LibraryLoading -> true
    is LibraryLoaded -> state.refreshing || state.books.any { it.id == bookId }
    is LibraryFailed -> state.books?.any { it.id == bookId } ?: true
}

fun reduceLibraryLoad(state: LibraryLoad, event: LibraryEvent): LibraryLoad = when (event) {
    is LibraryEvent.Cached -> when (state) {
        is LibraryLoading -> LibraryLoaded(event.books, refreshing = true)
        is LibraryLoaded -> if (state.books.isEmpty()) state.copy(books = event.books) else state
        is LibraryFailed -> state.copy(books = state.books ?: event.books)
    }
    LibraryEvent.RefreshStarted -> when (state) {
        is LibraryLoading -> state
        is LibraryLoaded -> state.copy(refreshing = true)
        is LibraryFailed -> state.books?.let { LibraryLoaded(it, refreshing = true) }
            ?: LibraryLoading
    }
    is LibraryEvent.RefreshSucceeded -> LibraryLoaded(event.books, refreshing = false)
    is LibraryEvent.RefreshFailed -> when (state) {
        is LibraryLoaded -> LibraryFailed(event.message, state.books)
        is LibraryFailed -> LibraryFailed(event.message, state.books)
        is LibraryLoading -> LibraryFailed(event.message, null)
    }
    is LibraryEvent.BooksReplaced -> when (state) {
        is LibraryLoading -> LibraryLoaded(event.books, refreshing = false)
        is LibraryLoaded -> state.copy(books = event.books)
        is LibraryFailed -> state.copy(books = event.books)
    }
    LibraryEvent.Dismissed -> when (state) {
        is LibraryFailed -> state.books?.let { LibraryLoaded(it, refreshing = false) } ?: state
        else -> state
    }
}
