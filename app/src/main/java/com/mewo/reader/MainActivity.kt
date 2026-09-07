package com.mewo.reader

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import com.mewo.reader.ui.MewoNav
import com.mewo.reader.ui.theme.MewoTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            /*
             * THESIS: A book is a timeline. Refuse the typeset page.
             * OWN-WORLD: Lights-out black, hairline rules, circular avatars,
             * blue/pink actions, Atkinson body. X's grammar, this product's text.
             * STORY: Import an EPUB, scroll the author as a feed, like a line.
             * FIRST VIEWPORT: Sticky "Mewo" bar, feed of books or empty timeline
             * plus one FAB to add a file.
             * FORM: X home timeline, brief-pinned, seed skipped.
             * FINISH: unreviewed and undocumented is unfinished; this build ends
             * with the finish review, the verdict, and DESIGN.md
             */
            MewoTheme {
                val app = application as MewoApp
                MewoNav(repository = app.repository)
            }
        }
    }
}
