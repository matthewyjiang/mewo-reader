package com.mewo.reader.data

/**
 * Where the timeline should begin.
 *
 * Do not trust EPUB start/bodymatter landmarks. Retail files point those at
 * the title page or a dedication. Do not wait for a "Chapter" title either:
 * many books use names, a prologue, or another language.
 *
 * Walk the TOC in order. Skip publisher chrome. The first entry that is not
 * chrome is the author starting to talk.
 */
internal fun feedStartHref(tocByHref: Map<String, String>): String? =
    tocByHref.entries.firstOrNull { !isPublisherChromeTitle(it.value) }?.key

internal fun isPublisherChromeTitle(title: String): Boolean {
    val t = normalizeTitle(title)
    if (t.isEmpty()) return false
    if (t in CHROME_TITLES) return true
    return CHROME_PREFIXES.any { t.startsWith(it) }
}

internal fun normalizeTitle(title: String): String =
    title.lowercase()
        .replace('’', '\'')
        .replace(Regex("[^a-z0-9]+"), " ")
        .trim()
        .replace(Regex("\\s+"), " ")

private val CHROME_TITLES = setOf(
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
)

private val CHROME_PREFIXES = listOf(
    "also by ",
    "other books ",
    "books by ",
)
