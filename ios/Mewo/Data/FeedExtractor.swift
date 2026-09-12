import Foundation
import ReadiumShared

/// Turns a Readium publication into timeline posts. One paragraph stays one post.
/// Heading posts come from HTML heading roles when Readium marks them, and from
/// the EPUB nav when it does not. Publisher chrome before the first non-chrome
/// TOC entry is dropped.
struct FeedExtractor: Sendable {
    func extract(_ publication: Publication) async throws -> [FeedPost] {
        guard let content = publication.content() else { return [] }
        let toc: [Link]
        switch await publication.tableOfContents() {
        case .success(let links):
            toc = links
        case .failure:
            toc = []
        }
        let tocByHref = tocTitlesByHref(toc)
        let startHref = feedStartHref(tocByHref: tocByHref)
        var seenTocHrefs = Set<String>()
        var posts: [FeedPost] = []
        var started = startHref == nil
        var chapter = "Start"
        var index = 0

        func add(kind: PostKind, text: String, chapterName: String) {
            posts.append(FeedPost(id: String(index), kind: kind, text: text, chapter: chapterName))
            index += 1
        }

        let elements = await content.elements()

        for element in elements {
            guard let textElement = element as? TextContentElement else { continue }
            let text = (textElement.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            if text.isEmpty { continue }

            let href = textElement.locator.href.string
            let key = hrefKey(href)
            if !started {
                if key == startHref {
                    started = true
                } else {
                    continue
                }
            }
            let tocTitle = tocByHref.first(where: { $0.key == key })?.value
            let tocStart = tocTitle != nil && !seenTocHrefs.contains(key)
            if tocStart { seenTocHrefs.insert(key) }

            let roleHeading = isHeadingRole(textElement.role)

            if tocStart, let tocTitle {
                chapter = tocTitle
                add(kind: .heading, text: tocTitle, chapterName: chapter)
                if sameHeading(text, tocTitle: tocTitle) { continue }
            } else if roleHeading {
                chapter = text
                add(kind: .heading, text: text, chapterName: chapter)
                continue
            }

            add(kind: .body, text: text, chapterName: chapter)
        }
        return posts
    }

    private func tocTitlesByHref(_ links: [Link]) -> [(key: String, value: String)] {
        var titles: [(key: String, value: String)] = []
        var seen = Set<String>()
        func walk(_ items: [Link]) {
            for link in items {
                let title = (link.title ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                let raw = link.url().string
                let key = hrefKey(raw)
                if !title.isEmpty && !key.isEmpty && !seen.contains(key) {
                    seen.insert(key)
                    titles.append((key: key, value: title))
                }
                walk(link.children)
            }
        }
        walk(links)
        return titles
    }

    private func isHeadingRole(_ role: TextContentElement.Role) -> Bool {
        if case .heading = role { return true }
        return false
    }
}
