package com.mewo.reader.ui.library

import androidx.compose.foundation.layout.Box
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier

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
 * One option runs [onUpload] immediately. Two options open a menu.
 * [trigger] is the control the reader taps; it receives the opener.
 */
@Composable
fun AddBookAnchor(
    options: List<AddBookOption>,
    onUpload: () -> Unit,
    onBrowseCatalog: () -> Unit,
    modifier: Modifier = Modifier,
    trigger: @Composable (open: () -> Unit) -> Unit,
) {
    if (options.size < 2) {
        Box(modifier) { trigger(onUpload) }
        return
    }
    var expanded by remember { mutableStateOf(false) }
    Box(modifier) {
        trigger { expanded = true }
        DropdownMenu(
            expanded = expanded,
            onDismissRequest = { expanded = false },
            containerColor = MaterialTheme.colorScheme.surface,
        ) {
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
}
