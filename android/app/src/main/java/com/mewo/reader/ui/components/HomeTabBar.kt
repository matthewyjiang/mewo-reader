package com.mewo.reader.ui.components

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.outlined.FavoriteBorder
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp

enum class AppTab { Home, Search, Likes }

@Composable
fun HomeTabBar(
    selected: AppTab,
    onSelect: (AppTab) -> Unit,
    modifier: Modifier = Modifier,
    visible: Boolean = true,
) {
    val ink = MaterialTheme.colorScheme.onBackground
    val mute = MaterialTheme.colorScheme.onSurfaceVariant
    Column(
        modifier = modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.background)
            .navigationBarsPadding(),
    ) {
        AnimatedVisibility(
            visible = visible,
            enter = ChromeMotion.bottomEnter(),
            exit = ChromeMotion.bottomExit(),
        ) {
            Column {
                HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(52.dp),
                    horizontalArrangement = Arrangement.SpaceEvenly,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TabIcon(
                        selected = selected == AppTab.Home,
                        selectedIcon = Icons.Filled.Home,
                        idleIcon = Icons.Outlined.Home,
                        label = "Home",
                        ink = ink,
                        mute = mute,
                        onClick = { onSelect(AppTab.Home) },
                    )
                    TabIcon(
                        selected = selected == AppTab.Search,
                        selectedIcon = Icons.Filled.Search,
                        idleIcon = Icons.Outlined.Search,
                        label = "Search",
                        ink = ink,
                        mute = mute,
                        onClick = { onSelect(AppTab.Search) },
                    )
                    TabIcon(
                        selected = selected == AppTab.Likes,
                        selectedIcon = Icons.Filled.Favorite,
                        idleIcon = Icons.Outlined.FavoriteBorder,
                        label = "Likes",
                        ink = ink,
                        mute = mute,
                        onClick = { onSelect(AppTab.Likes) },
                    )
                }
            }
        }
    }
}

@Composable
private fun TabIcon(
    selected: Boolean,
    selectedIcon: ImageVector,
    idleIcon: ImageVector,
    label: String,
    ink: Color,
    mute: Color,
    onClick: () -> Unit,
) {
    IconButton(onClick = onClick, modifier = Modifier.size(48.dp)) {
        Icon(
            imageVector = if (selected) selectedIcon else idleIcon,
            contentDescription = label,
            tint = if (selected) ink else mute,
            modifier = Modifier.size(26.dp),
        )
    }
}
