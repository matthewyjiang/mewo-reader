package com.mewo.reader.ui.reader

data class LinkSpan(val start: Int, val end: Int, val href: String)

private const val TRAILING = ".,;:!?)]}'\""

/**
 * Same rule as web `linkSpans`. A match starts at `http://` or `https://`
 * even when the previous character is not a space, ends at whitespace, and
 * drops trailing `.,;:!?)]}'"` except a `)` that closes an unmatched `(`.
 * A trailing `/` stays. Indexes are UTF-16 code units.
 */
fun linkSpans(text: String): List<LinkSpan> {
    val spans = mutableListOf<LinkSpan>()
    var cursor = 0
    while (cursor < text.length) {
        val https = text.indexOf("https://", cursor)
        val http = text.indexOf("http://", cursor)
        val start = when {
            https < 0 -> http
            http < 0 -> https
            else -> minOf(https, http)
        }
        if (start < 0) break
        val schemeLength = if (text.startsWith("https://", start)) "https://".length else "http://".length
        var end = start
        while (end < text.length && !text[end].isWhitespace()) end += 1
        while (end > start + schemeLength && text[end - 1] in TRAILING) {
            if (text[end - 1] == ')' && unmatchedOpen(text, start, end - 1)) break
            end -= 1
        }
        if (end > start + schemeLength) {
            spans.add(LinkSpan(start, end, text.substring(start, end)))
            cursor = end
        } else {
            cursor = start + schemeLength
        }
    }
    return spans
}

private fun unmatchedOpen(text: String, start: Int, end: Int): Boolean {
    var open = 0
    var close = 0
    for (index in start until end) {
        when (text[index]) {
            '(' -> open += 1
            ')' -> close += 1
        }
    }
    return open > close
}
