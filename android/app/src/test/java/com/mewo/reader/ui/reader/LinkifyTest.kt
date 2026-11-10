package com.mewo.reader.ui.reader

import org.junit.Assert.assertEquals
import org.junit.Test

class LinkifyTest {
    @Test
    fun aUrlGluedToThePreviousWordIsALinkAndTrailingPunctuationIsNot() {
        val text = "supervision:https://en.wikipedia.org/wiki/Hopfield_network; https://en.wikipedia.org/wiki/Backpropagation."
        val spans = linkSpans(text)
        assertEquals(
            listOf(
                "https://en.wikipedia.org/wiki/Hopfield_network",
                "https://en.wikipedia.org/wiki/Backpropagation",
            ),
            spans.map { it.href },
        )
        assertEquals(text.substring(spans[0].start, spans[0].end), spans[0].href)
        assertEquals(';', text[spans[0].end])
        assertEquals('.', text[spans[1].end])
    }

    @Test
    fun aTrailingSlashStaysAndTheSentencePeriodDoesNot() {
        val text = "microbes:https://www.ncbi.nlm.nih.gov/pmc/articles/PMC4790845/."
        val spans = linkSpans(text)
        assertEquals(1, spans.size)
        assertEquals("microbes:".length, spans[0].start)
        assertEquals(text.length - 1, spans[0].end)
        assertEquals("https://www.ncbi.nlm.nih.gov/pmc/articles/PMC4790845/", spans[0].href)
    }

    @Test
    fun proseAroundAUrlStaysOutsideTheLinkAndTheSlashStays() {
        val text = "go to https://www.ncbi.nlm.nih.gov/pmc/articles/ and at the search bar"
        val spans = linkSpans(text)
        assertEquals(1, spans.size)
        assertEquals("https://www.ncbi.nlm.nih.gov/pmc/articles/", spans[0].href)
        assertEquals("go to ", text.substring(0, spans[0].start))
        assertEquals(" and at the search bar", text.substring(spans[0].end))
    }

    @Test
    fun anUnmatchedParenthesisInsideTheUrlIsKeptAndTheWrappingOneIsNot() {
        val text = "(https://example.com/wiki/Hopfield_network_(neural))."
        val spans = linkSpans(text)
        assertEquals(listOf("https://example.com/wiki/Hopfield_network_(neural)"), spans.map { it.href })
        assertEquals('(', text[spans[0].start - 1])
        assertEquals(").", text.substring(spans[0].end))
    }

    @Test
    fun aLabelThatIsNotAUrlIsLeftAlone() {
        assertEquals(emptyList<LinkSpan>(), linkSpans("See this paper."))
    }

    @Test
    fun anIpv6UrlKeepsItsClosingBracket() {
        val text = "http://[::1]"
        assertEquals(listOf(LinkSpan(0, text.length, text)), linkSpans(text))
        val dotted = "see http://[::1]."
        val spans = linkSpans(dotted)
        assertEquals("http://[::1]", spans.single().href)
        assertEquals('.', dotted[spans[0].end])
        assertEquals("http://example.com/foo", linkSpans("http://example.com/foo]").single().href)
    }
}
