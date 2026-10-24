package com.mewo.reader.ui.components

import com.mewo.reader.data.OfflineSummary
import com.mewo.reader.ui.library.downloadingLabel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DownloadCopyTest {
    @Test
    fun settingsRowNamesTheThreeStates() {
        assertEquals("Download all books", downloadAllLabel(OfflineSummary(0, 12, running = false)))
        assertTrue(downloadAllEnabled(OfflineSummary(0, 12, running = false)))
        assertEquals("Downloading 3 of 12", downloadAllLabel(OfflineSummary(3, 12, running = true)))
        assertTrue(downloadAllEnabled(OfflineSummary(3, 12, running = true)))
        assertEquals("All books downloaded", downloadAllLabel(OfflineSummary(12, 12, running = false)))
        assertFalse(downloadAllEnabled(OfflineSummary(12, 12, running = false)))
        assertEquals("All books downloaded", downloadAllLabel(OfflineSummary(0, 0, running = false)))
    }

    @Test
    fun theRingNamesAPercentOnlyWhenTheLengthIsKnown() {
        assertEquals("Downloading", downloadingLabel(null))
        assertEquals("Downloading, 40%", downloadingLabel(0.4f))
        assertEquals("Downloading, 0%", downloadingLabel(0f))
        assertEquals("Downloading, 100%", downloadingLabel(1f))
    }
}
