package com.mewo.reader.ui.components

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.LocalIdentity

@Composable
fun ProfilePreview(
    name: String,
    handle: String,
    modifier: Modifier = Modifier,
) {
    val display = name.trim().ifBlank { LocalIdentity.normalizeHandle(handle).ifBlank { "Name" } }
    val tag = LocalIdentity.normalizeHandle(handle)
    val named = name.trim().isNotEmpty()
    val handled = tag.isNotEmpty()
    Column(modifier = modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.padding(vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Avatar(name = display, size = 40.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    text = display,
                    style = MaterialTheme.typography.titleSmall,
                    color = if (named || handled) {
                        MaterialTheme.colorScheme.onBackground
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    text = if (handled) "@$tag" else "@handle",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
    }
}

@Composable
fun ProfileForm(
    name: String,
    handle: String,
    onNameChange: (String) -> Unit,
    onHandleChange: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val nameError = LocalIdentity.nameError(name)
    val handleError = LocalIdentity.handleError(handle)
    val fieldColors = OutlinedTextFieldDefaults.colors(
        focusedBorderColor = MaterialTheme.colorScheme.primary,
        unfocusedBorderColor = MaterialTheme.colorScheme.onSurfaceVariant,
        focusedContainerColor = Color.Transparent,
        unfocusedContainerColor = Color.Transparent,
        errorBorderColor = MaterialTheme.colorScheme.error,
        focusedTextColor = MaterialTheme.colorScheme.onBackground,
        unfocusedTextColor = MaterialTheme.colorScheme.onBackground,
        focusedLabelColor = MaterialTheme.colorScheme.primary,
        unfocusedLabelColor = MaterialTheme.colorScheme.onSurfaceVariant,
        cursorColor = MaterialTheme.colorScheme.primary,
    )
    Column(modifier = modifier.fillMaxWidth()) {
        OutlinedTextField(
            value = name,
            onValueChange = onNameChange,
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            isError = nameError != null,
            label = { Text("Name") },
            placeholder = { Text("Optional") },
            supportingText = nameError?.let { { Text(it) } },
            keyboardOptions = KeyboardOptions(
                capitalization = KeyboardCapitalization.Words,
                imeAction = ImeAction.Next,
            ),
            textStyle = MaterialTheme.typography.bodyMedium,
            colors = fieldColors,
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = handle,
            onValueChange = { onHandleChange(it.trimStart('@')) },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            isError = handleError != null,
            label = { Text("Handle") },
            prefix = { Text("@") },
            supportingText = handleError?.let { { Text(it) } },
            keyboardOptions = KeyboardOptions(
                capitalization = KeyboardCapitalization.None,
                keyboardType = KeyboardType.Ascii,
                imeAction = ImeAction.Done,
            ),
            textStyle = MaterialTheme.typography.bodyMedium,
            colors = fieldColors,
        )
    }
}
