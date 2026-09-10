package com.mewo.reader.ui.components

import androidx.compose.material3.IconButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/** Dummy account photo. Opens the X-style account drawer. */
@Composable
fun ProfileButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    IconButton(
        onClick = onClick,
        modifier = modifier.semantics { contentDescription = "Account" },
    ) {
        Avatar(name = "You", size = 32.dp)
    }
}
