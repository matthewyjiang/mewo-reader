@file:OptIn(org.readium.r2.shared.ExperimentalReadiumApi::class)

package com.mewo.reader.data

import org.readium.r2.shared.publication.Link
import org.readium.r2.shared.publication.Publication
import org.readium.r2.shared.publication.services.content.Content
import org.readium.r2.shared.publication.services.content.content

/**
 * Turns a Readium publication into timeline posts. One paragraph stays one post.
 * Heading posts come from HTML heading roles when Readium marks them, and from
 * the EPUB nav when it does not: the first text in a TOC href becomes that entry.
 * Publisher chrome before the first non-chrome TOC entry is dropped.
 */
class FeedExtractor {
    suspend fun extract(publication: Publication): List<FeedPost> {
        val content = publication.content() ?: return emptyList()
        val tocByHref = tocTitlesByHref(publication.tableOfContents)
        val startHref = feedStartHref(tocByHref)
        val seenTocHrefs = mutableSetOf<String>()
        val posts = mutableListOf<FeedPost>()
        var started = startHref == null
        var chapter = "Start"
        var index = 0

        fun add(kind: PostKind, text: String, chapterName: String) {
            posts += FeedPost(
                id = index.toString(),
                kind = kind,
                text = text,
                chapter = chapterName,
            )
            index += 1
        }

        for (element in content.elements()) {
            val textElement = element as? Content.TextElement ?: continue
            val text = textElement.text.trim()
            if (text.isEmpty()) continue

            val hrefKey = hrefKey(textElement.locator.href.toString())
            if (!started) {
                if (hrefKey == startHref) started = true
                else continue
            }
            val tocTitle = tocByHref[hrefKey]
            val tocStart = tocTitle != null && hrefKey !in seenTocHrefs
            if (tocStart) seenTocHrefs += hrefKey

            val roleHeading = textElement.role is Content.TextElement.Role.Heading

            if (tocStart && tocTitle != null) {
                chapter = tocTitle
                add(PostKind.Heading, tocTitle, chapter)
                if (sameHeading(text, tocTitle)) continue
            } else if (roleHeading) {
                chapter = text
                add(PostKind.Heading, text, chapter)
                continue
            }

            add(PostKind.Body, text, chapter)
        }
        return posts
    }

    private fun tocTitlesByHref(links: List<Link>): Map<String, String> {
        val titles = linkedMapOf<String, String>()
        fun walk(items: List<Link>) {
            for (link in items) {
                val title = link.title?.trim().orEmpty()
                val key = hrefKey(link.url()?.toString() ?: link.href.toString())
                if (title.isNotEmpty() && key.isNotEmpty()) {
                    titles.putIfAbsent(key, title)
                }
                walk(link.children)
            }
        }
        walk(links)
        return titles
    }
}

internal fun hrefKey(raw: String): String =
    raw.substringBefore('#').substringBefore('?').trim('/').substringAfterLast('/').lowercase()

internal fun sameHeading(text: String, tocTitle: String): Boolean {
    fun norm(value: String): String = value.lowercase().replace(Regex("[^a-z0-9]+"), "")
    val body = norm(text)
    val title = norm(tocTitle)
    if (body.isEmpty() || title.isEmpty()) return false
    if (body == title) return true
    if (body == title.removePrefix("chapter")) return true
    if (body == title.removePrefix("part")) return true
    return false
}
