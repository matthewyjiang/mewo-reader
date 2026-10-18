package com.mewo.reader.ui.reader

import org.junit.Assert.assertEquals
import org.junit.Test

class RelativeTimeTest {
    private val now = 1_792_281_600_000L // 2026-10-18T00:00:00Z

    @Test
    fun aRecentCommentStaysShortAndAnOlderOneIsADate() {
        assertEquals("now", relativeTime(now, now))
        assertEquals("now", relativeTime(now + 60_000, now))
        assertEquals("5m", relativeTime(now - 5 * 60_000, now))
        assertEquals("3h", relativeTime(now - 3 * 3_600_000, now))
        assertEquals("23h", relativeTime(now - 23 * 3_600_000, now))
        assertEquals("Oct 3", relativeTime(1_791_039_600_000L, now)) // 2026-10-03T15:00:00Z
        assertEquals("Oct 17", relativeTime(now - 24 * 3_600_000, now))
        assertEquals("Oct 3, 2024", relativeTime(1_727_967_600_000L, now))
        assertEquals("Jan 1, 1970", relativeTime(5, now))
    }
}
