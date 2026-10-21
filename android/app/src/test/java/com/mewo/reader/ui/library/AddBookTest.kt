package com.mewo.reader.ui.library

import org.junit.Assert.assertEquals
import org.junit.Test

class AddBookTest {
    @Test
    fun catalogIsASecondChoiceOnlyWhenRequestsAreOn() {
        assertEquals(listOf(AddBookOption.Upload), addBookOptions(canRequest = false))
        assertEquals(
            listOf(AddBookOption.Upload, AddBookOption.Browse),
            addBookOptions(canRequest = true),
        )
        assertEquals("Upload EPUB", AddBookOption.Upload.label)
        assertEquals("Browse catalog", AddBookOption.Browse.label)
    }
}
