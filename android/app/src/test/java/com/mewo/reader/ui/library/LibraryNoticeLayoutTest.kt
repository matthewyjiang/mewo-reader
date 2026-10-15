package com.mewo.reader.ui.library

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import com.mewo.reader.ui.theme.DisplayTheme
import com.mewo.reader.ui.theme.mewoTypography
import com.mewo.reader.ui.theme.toColorScheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33], qualifiers = "w320dp-mdpi")
class LibraryNoticeLayoutTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun messageAndActionStayCenteredAsTheMessageWraps() = assertCentered(fontScale = 1f)

    @Test
    fun largeTextKeepsEvenVerticalPadding() = assertCentered(fontScale = 1.3f)

    private fun assertCentered(fontScale: Float) {
        val messages = listOf(
            "Book imported.",
            "This book is private.\nUse Share on Home.",
            "Slouching Towards Bethlehem is private. Use Share on Home when you want others to read it.",
        )
        compose.setContent {
            val palette = DisplayTheme.XLight.palette
            val density = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(density.density, fontScale)) {
                MaterialTheme(
                    colorScheme = palette.toColorScheme(),
                    typography = mewoTypography(palette.ink),
                ) {
                    Column(Modifier.width(320.dp)) {
                        messages.forEachIndexed { index, message ->
                            LibraryNotice(
                                message = message,
                                onDismiss = {},
                                modifier = Modifier.testTag("notice-$index"),
                            )
                        }
                    }
                }
            }
        }

        val actions = compose.onAllNodesWithText("OK").fetchSemanticsNodes()
        messages.forEachIndexed { index, message ->
            val banner = compose.onNodeWithTag("notice-$index").fetchSemanticsNode().boundsInRoot
            val text = compose.onNodeWithText(message).fetchSemanticsNode().boundsInRoot
            assertEquals("Message padding: $message", banner.center.y, text.center.y, 0.5f)
            assertEquals("Action alignment: $message", banner.center.y, actions[index].boundsInRoot.center.y, 0.5f)
        }
    }
}
