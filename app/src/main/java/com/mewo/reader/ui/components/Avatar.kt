package com.mewo.reader.ui.components

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.mewo.reader.ui.theme.Atkinson
import com.mewo.reader.ui.theme.AvatarPalette
import com.mewo.reader.ui.theme.XPaper
import java.io.File

@Composable
fun Avatar(
    name: String,
    coverFile: File? = null,
    size: Dp = 40.dp,
    modifier: Modifier = Modifier,
) {
    val bitmap = remember(coverFile?.path, coverFile?.lastModified()) {
        coverFile
            ?.takeIf { it.exists() }
            ?.let { BitmapFactory.decodeFile(it.absolutePath)?.asImageBitmap() }
    }
    val color = remember(name) {
        val idx = (name.hashCode() and 0x7fffffff) % AvatarPalette.size
        AvatarPalette[idx]
    }
    val letter = name.trim().firstOrNull()?.uppercaseChar()?.toString() ?: "M"

    Box(
        modifier = modifier
            .size(size)
            .clip(CircleShape)
            .background(color),
        contentAlignment = Alignment.Center,
    ) {
        if (bitmap != null) {
            Image(
                bitmap = bitmap,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.matchParentSize(),
            )
        } else {
            Text(
                text = letter,
                color = XPaper,
                fontFamily = Atkinson,
                fontWeight = FontWeight.Bold,
                fontSize = (size.value * 0.42f).sp,
            )
        }
    }
}
