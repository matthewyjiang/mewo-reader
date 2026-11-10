package com.mewo.reader.ui.reader

import androidx.activity.ComponentActivity
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.UriHandler
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performFirstLinkClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.geometry.Offset
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.PostKind
import com.mewo.reader.ui.theme.LocalReaderFont
import com.mewo.reader.ui.theme.ReaderFontController
import com.mewo.reader.ui.theme.ReaderTypeScale
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [33])
class TimelinePostLinkTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun aLinkTapOpensTheUrlAndDoesNotOpenThePost() {
        val opened = mutableListOf<String>()
        var opens = 0
        val handler = object : UriHandler {
            override fun openUri(uri: String) {
                opened.add(uri)
            }
        }
        compose.setContent {
            CompositionLocalProvider(
                LocalReaderFont provides ReaderFontController(ReaderTypeScale.Default) {},
                LocalUriHandler provides handler,
            ) {
                MaterialTheme {
                    TimelinePost(
                        post = FeedPost(
                            id = "0",
                            kind = PostKind.Body,
                            text = "see https://example.com/x now",
                            chapter = "Notes",
                        ),
                        author = "Ada",
                        handle = "ada",
                        cover = null,
                        liked = false,
                        onComment = {},
                        onLike = {},
                        onRepost = {},
                        onShare = {},
                        onOpen = { opens += 1 },
                    )
                }
            }
        }

        compose.onNodeWithText("see https://example.com/x now").performFirstLinkClick()
        assertEquals(listOf("https://example.com/x"), opened)
        assertEquals(0, opens)

        val body = compose.onNodeWithText("see https://example.com/x now")
        body.performTouchInput {
            click(Offset(1f, height / 2f))
        }
        assertEquals(1, opens)
        assertEquals(listOf("https://example.com/x"), opened)
    }
}
