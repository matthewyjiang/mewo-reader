package com.mewo.reader.ui.components

import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import com.mewo.reader.R
import com.mewo.reader.ui.theme.DisplayTheme
import com.mewo.reader.ui.theme.LocalMewoColors
import com.mewo.reader.ui.theme.LocalThemeController
import com.mewo.reader.ui.theme.TwitterBlue

/**
 * Home-timeline mark. Twitter bird or the current X, tinted like the real apps.
 */
@Composable
fun BrandMark(modifier: Modifier = Modifier) {
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
