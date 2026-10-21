package com.mewo.reader.ui.library

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

enum class AddBookOption(val label: String) {
    Upload("Upload EPUB"),
    Browse("Browse catalog"),
}

fun addBookOptions(canRequest: Boolean): List<AddBookOption> {
    return if (canRequest) {
        listOf(AddBookOption.Upload, AddBookOption.Browse)
    } else {
        listOf(AddBookOption.Upload)
    }
}

/**
 * One option runs [onUpload] immediately. Two options open a menu above the
 * control. The menu is in this layout, not a popup window, so a tap on it
 * lands and a screenshot of the screen includes it.
 * [trigger] is the control the reader taps; it receives the opener.
 */
@Composable
fun AddBookAnchor(
    options: List<AddBookOption>,
    onUpload: () -> Unit,
    onBrowseCatalog: () -> Unit,
    modifier: Modifier = Modifier,
    menuAlignment: Alignment.Horizontal = Alignment.Start,
    startExpanded: Boolean = false,
    trigger: @Composable (open: () -> Unit) -> Unit,
) {
    if (options.size < 2) {
        Box(modifier) { trigger(onUpload) }
        return
    }
    var expanded by remember { mutableStateOf(startExpanded) }
    BackHandler(enabled = expanded) { expanded = false }
    Column(modifier, horizontalAlignment = menuAlignment) {
        if (expanded) {
            Surface(
                shape = RoundedCornerShape(4.dp),
                color = MaterialTheme.colorScheme.surface,
                shadowElevation = 3.dp,
            ) {
                Column(Modifier.widthIn(min = 112.dp, max = 280.dp)) {
                    options.forEach { option ->
                        DropdownMenuItem(
                            text = { Text(option.label) },
                            onClick = {
                                expanded = false
                                when (option) {
                                    AddBookOption.Upload -> onUpload()
                                    AddBookOption.Browse -> onBrowseCatalog()
                                }
                            },
                        )
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
        }
        trigger { expanded = !expanded }
    }
}
