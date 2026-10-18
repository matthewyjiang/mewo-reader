import Foundation

/// Where a shelf fetch stands. Empty copy is a finished fetch that returned nothing,
/// never the gap before the first response.
enum LibraryLoad: Equatable, Sendable {
    /// No fetch has finished, and there is nothing saved to show.
    case loading
    /// `books` came from a successful fetch. `refreshing` is a background reload.
    case loaded(books: [BookRecord], refreshing: Bool)
    /// The last fetch failed. `books` is the previous success, or nil when this account has never loaded.
    case failed(message: String, books: [BookRecord]?)

    /// Saved rows, if a fetch has succeeded at least once.
    var cachedBooks: [BookRecord]? {
        switch self {
        case .loading:
            return nil
        case .loaded(let books, _):
            return books
        case .failed(_, let books):
            return books
        }
    }

    var books: [BookRecord] { cachedBooks ?? [] }

    /// Empty copy is only for a finished fetch that returned nothing.
    var showEmpty: Bool { cachedBooks?.isEmpty == true }

    var isRefreshing: Bool {
        if case .loaded(_, true) = self { return true }
        return false
    }
}

enum LibraryEvent: Equatable, Sendable {
    case cached([BookRecord])
    case refreshStarted
    case refreshSucceeded([BookRecord])
    case refreshFailed(String)
    case booksReplaced([BookRecord])
    case dismissed
}

func reduceLibraryLoad(_ state: LibraryLoad, _ event: LibraryEvent) -> LibraryLoad {
    switch event {
    case .cached(let books):
        switch state {
        case .loading:
            return .loaded(books: books, refreshing: true)
        case .loaded(let current, let refreshing):
            if current.isEmpty { return .loaded(books: books, refreshing: refreshing) }
            return state
        case .failed(let message, let current):
            return .failed(message: message, books: current ?? books)
        }
    case .refreshStarted:
        switch state {
        case .loading:
            return state
        case .loaded(let books, _):
            return .loaded(books: books, refreshing: true)
        case .failed(_, let books):
            if let books { return .loaded(books: books, refreshing: true) }
            return .loading
        }
    case .refreshSucceeded(let books):
        return .loaded(books: books, refreshing: false)
    case .refreshFailed(let message):
        switch state {
        case .loaded(let books, _):
            return .failed(message: message, books: books)
        case .failed(_, let books):
            return .failed(message: message, books: books)
        case .loading:
            return .failed(message: message, books: nil)
        }
    case .booksReplaced(let books):
        switch state {
        case .loading:
            return .loaded(books: books, refreshing: false)
        case .loaded(_, let refreshing):
            return .loaded(books: books, refreshing: refreshing)
        case .failed(let message, _):
            return .failed(message: message, books: books)
        }
    case .dismissed:
        switch state {
        case .failed(_, let books):
            if let books { return .loaded(books: books, refreshing: false) }
            return state
        default:
            return state
        }
    }
}

/// Saved text may open a book only while the shelf is still catching up, or when a
/// finished shelf still lists it. A completed refresh is the list.
func savedTextAllowed(_ state: LibraryLoad, bookId: String) -> Bool {
    switch state {
    case .loading:
        return true
    case .loaded(_, true):
        return true
    case .failed(_, nil):
        return true
    default:
        return state.books.contains { $0.id == bookId }
    }
}
