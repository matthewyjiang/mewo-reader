import Foundation

/// Where a home-feed fetch stands. Empty copy is a finished fetch that returned nothing.
enum FeedLoad: Equatable, Sendable {
    case loading
    case loaded(items: [HomeFeedItem], refreshing: Bool, nextCursor: String?)
    case failed(message: String, items: [HomeFeedItem]?, nextCursor: String?)
    case signedOut

    var items: [HomeFeedItem] {
        switch self {
        case .loading, .signedOut:
            return []
        case .loaded(let items, _, _):
            return items
        case .failed(_, let items, _):
            return items ?? []
        }
    }

    var nextCursor: String? {
        switch self {
        case .loaded(_, _, let cursor):
            return cursor
        case .failed(_, _, let cursor):
            return cursor
        case .loading, .signedOut:
            return nil
        }
    }

    /// Empty copy is only for a finished fetch that returned nothing.
    var showEmpty: Bool {
        if case .loaded(let items, _, _) = self { return items.isEmpty }
        return false
    }

    var isRefreshing: Bool {
        if case .loaded(_, true, _) = self { return true }
        return false
    }

    var failureMessage: String? {
        if case .failed(let message, let items, _) = self, items != nil {
            return message
        }
        return nil
    }

    var isFailedEmpty: Bool {
        if case .failed(_, nil, _) = self { return true }
        return false
    }
}

enum FeedEvent: Equatable, Sendable {
    case cached([HomeFeedItem], String?)
    case refreshStarted
    case refreshSucceeded([HomeFeedItem], String?)
    case refreshFailed(String)
    case pageAppended([HomeFeedItem], String?)
    case signedOut
}

func reduceFeedLoad(_ state: FeedLoad, _ event: FeedEvent) -> FeedLoad {
    switch event {
    case .cached(let items, let cursor):
        switch state {
        case .loading:
            return .loaded(items: items, refreshing: true, nextCursor: cursor)
        case .loaded(let current, let refreshing, let currentCursor):
            if current.isEmpty { return .loaded(items: items, refreshing: refreshing, nextCursor: cursor) }
            return .loaded(items: current, refreshing: refreshing, nextCursor: currentCursor)
        case .failed(let message, let current, let currentCursor):
            return .failed(message: message, items: current ?? items, nextCursor: currentCursor ?? cursor)
        case .signedOut:
            return state
        }
    case .refreshStarted:
        switch state {
        case .loading:
            return state
        case .loaded(let items, _, let cursor):
            return .loaded(items: items, refreshing: true, nextCursor: cursor)
        case .failed(_, let items, let cursor):
            if let items { return .loaded(items: items, refreshing: true, nextCursor: cursor) }
            return .loading
        case .signedOut:
            return .loading
        }
    case .refreshSucceeded(let items, let cursor):
        return .loaded(items: items, refreshing: false, nextCursor: cursor)
    case .refreshFailed(let message):
        switch state {
        case .loaded(let items, _, let cursor):
            return .failed(message: message, items: items, nextCursor: cursor)
        case .failed(_, let items, let cursor):
            return .failed(message: message, items: items, nextCursor: cursor)
        case .loading, .signedOut:
            return .failed(message: message, items: nil, nextCursor: nil)
        }
    case .pageAppended(let more, let cursor):
        switch state {
        case .loaded(let items, let refreshing, _):
            return .loaded(items: items + more, refreshing: refreshing, nextCursor: cursor)
        case .failed(let message, let items, _):
            return .failed(message: message, items: (items ?? []) + more, nextCursor: cursor)
        case .loading, .signedOut:
            return .loaded(items: more, refreshing: false, nextCursor: cursor)
        }
    case .signedOut:
        return .signedOut
    }
}
