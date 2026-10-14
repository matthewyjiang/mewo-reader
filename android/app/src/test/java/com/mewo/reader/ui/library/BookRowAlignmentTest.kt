package com.mewo.reader.ui.library

import android.graphics.Bitmap
import android.graphics.Canvas
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.BookRecord
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [33], qualifiers = "w360dp-mdpi")
class BookRowAlignmentTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun shareAndRemoveMatchAndCenterOnTheRow() {
        compose.setContent {
            MaterialTheme {
                Column(Modifier.width(360.dp).testTag("rows")) {
                    HostedRow(shared = false, owner = null, tag = "private-row")
                    HostedRow(shared = true, owner = null, tag = "shared-row")
                    HostedRow(shared = false, owner = "ada", tag = "tall-row")
                }
            }
        }

        assertAligned("private-row", "Share")
        assertAligned("shared-row", "Make private")
        assertAligned("tall-row", "Share")

        save("hosted_book_row_actions.png")
    }

    private fun assertAligned(tag: String, shareLabel: String) {
        val row = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
        val share = boundsInside(shareLabel, row)
        val remove = boundsInside("Remove Pride and Prejudice", row)
        assertEquals(share.width, remove.width, 0.5f)
        assertEquals(share.height, remove.height, 0.5f)
        assertEquals(share.center.y, remove.center.y, 0.5f)
        assertEquals(row.center.y, share.center.y, 0.5f)
        assertEquals(row.center.y, remove.center.y, 0.5f)
    }

    private fun boundsInside(description: String, row: Rect): Rect {
        val matches = compose.onAllNodesWithContentDescription(description)
            .fetchSemanticsNodes()
            .map { it.boundsInRoot }
            .filter { it.top >= row.top - 1f && it.bottom <= row.bottom + 1f }
        assertEquals(description, 1, matches.size)
        return matches.single()
    }

    private fun save(name: String) {
        compose.waitForIdle()
        val view = compose.activity.window.decorView
        val full = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        view.draw(Canvas(full))
        val bounds = compose.onNodeWithTag("rows").fetchSemanticsNode().boundsInRoot
        val left = bounds.left.toInt().coerceIn(0, full.width - 1)
        val top = bounds.top.toInt().coerceIn(0, full.height - 1)
        val right = bounds.right.toInt().coerceIn(left + 1, full.width)
        val bottom = bounds.bottom.toInt().coerceIn(top + 1, full.height)
        val bitmap = Bitmap.createBitmap(full, left, top, right - left, bottom - top)
        val dir = File("/opt/cursor/artifacts")
        if (!dir.isDirectory) return
        File(dir, name).outputStream().use { out ->
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
        }
    }
}

@Composable
private fun HostedRow(shared: Boolean, owner: String?, tag: String) {
    BookPost(
        book = BookRecord(
            id = tag,
            title = "Pride and Prejudice",
            author = "Austen",
            handle = "austen",
            importedAt = 1,
            mine = true,
            shared = shared,
            ownerUsername = owner,
        ),
        cover = null,
        showOwner = owner != null,
        onOpen = {},
        onDelete = {},
        onShare = {},
        modifier = Modifier.testTag(tag),
    )
}
