package com.mewo.reader.ui.components

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import com.mewo.reader.R
import com.mewo.reader.ui.theme.DisplayTheme
import com.mewo.reader.ui.theme.LocalMewoColors
import com.mewo.reader.ui.theme.LocalMewoMode
import com.mewo.reader.ui.theme.LocalThemeController
import com.mewo.reader.ui.theme.TwitterBlue

/**
 * Home-timeline mark. Teddy in Mewo mode, else Twitter bird or X.
 */
@Composable
fun BrandMark(modifier: Modifier = Modifier) {
    if (LocalMewoMode.current.enabled) {
        Image(
            painter = painterResource(R.drawable.mewo_teddy),
            contentDescription = "Mewo",
            contentScale = ContentScale.Fit,
            modifier = modifier.size(28.dp),
        )
        return
    }
    val theme = LocalThemeController.current.current
    val palette = LocalMewoColors.current
    val twitter = theme == DisplayTheme.TwitterLight || theme == DisplayTheme.TwitterDark
    val tint = if (twitter && !palette.isDark) TwitterBlue else palette.ink
    Icon(
        painter = painterResource(
            if (twitter) R.drawable.ic_brand_twitter else R.drawable.ic_brand_x,
        ),
        contentDescription = "Mewo",
        tint = tint,
        modifier = modifier.size(
            width = if (twitter) 28.dp else 24.dp,
            height = if (twitter) 23.dp else 22.dp,
        ),
    )
}
