import Foundation

/// Where the timeline should begin.
///
/// Do not trust EPUB start/bodymatter landmarks. Retail files point those at
/// the title page or a dedication. Walk the TOC in order. Skip publisher
/// chrome. The first entry that is not chrome is the author starting to talk.
func feedStartHref(tocByHref: [(key: String, value: String)]) -> String? {
    tocByHref.first { !isPublisherChromeTitle($0.value) }?.key
}

func isPublisherChromeTitle(_ title: String) -> Bool {
    let t = normalizeTitle(title)
    if t.isEmpty { return false }
    if chromeTitles.contains(t) { return true }
    return chromePrefixes.contains { t.hasPrefix($0) }
}

func normalizeTitle(_ title: String) -> String {
    title.lowercased()
        .replacingOccurrences(of: "’", with: "'")
        .replacingOccurrences(of: "[^a-z0-9]+", with: " ", options: .regularExpression)
        .trimmingCharacters(in: .whitespacesAndNewlines)
        .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
}

func hrefKey(_ raw: String) -> String {
    let noFrag = raw.split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? raw
    let noQuery = noFrag.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? noFrag
    return (noQuery as NSString).lastPathComponent
        .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        .lowercased()
}

func sameHeading(_ text: String, tocTitle: String) -> Bool {
    func norm(_ value: String) -> String {
        value.lowercased().replacingOccurrences(of: "[^a-z0-9]+", with: "", options: .regularExpression)
    }
    let body = norm(text)
    let title = norm(tocTitle)
    if body.isEmpty || title.isEmpty { return false }
    if body == title { return true }
    if body == String(title.dropFirst("chapter".count)) && title.hasPrefix("chapter") { return true }
    if body == String(title.dropFirst("part".count)) && title.hasPrefix("part") { return true }
    return false
}

private let chromeTitles: Set<String> = [
    "cover",
    "title page",
    "copyright",
    "copyright page",
    "contents",
    "table of contents",
    "toc",
    "other books by this author",
    "other books",
    "also by this author",
    "also by",
    "books by this author",
    "by the same author",
    "half title",
    "colophon",
    "frontispiece",
    "imprint",
    "about the publisher",
    "publisher s note",
    "publishers note",
    "translator s note",
    "translators note",
    "about this edition",
    "about the ebook",
    "about the e book",
]

private let chromePrefixes: [String] = [
    "also by ",
    "other books ",
    "books by ",
]
