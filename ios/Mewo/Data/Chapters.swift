import Foundation

extension Array where Element == FeedPost {
    func headingPosts() -> [FeedPost] {
        filter { $0.kind == .heading }
    }
}

/// Index in headings of the last heading at or before visibleIndex in posts.
func currentHeadingIndex(posts: [FeedPost], headings: [FeedPost], visibleIndex: Int) -> Int {
    guard !headings.isEmpty else { return 0 }
    var indexById: [String: Int] = [:]
    indexById.reserveCapacity(posts.count)
    for (index, post) in posts.enumerated() {
        indexById[post.id] = index
    }
    let limit = visibleIndex
    return headings.indices.last(where: { i in
        (indexById[headings[i].id] ?? Int.max) <= limit
    }) ?? 0
}
