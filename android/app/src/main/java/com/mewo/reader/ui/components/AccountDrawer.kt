package com.mewo.reader.ui.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.systemGestureExclusion
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.DrawerState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.mewo.reader.ui.currentReaderIdentity

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountDrawer(
    drawerState: DrawerState,
    gesturesEnabled: Boolean,
    onOpenSettings: () -> Unit,
    onOpenProfile: (handle: String, name: String) -> Unit,
    content: @Composable () -> Unit,
) {
    ModalNavigationDrawer(
        drawerState = drawerState,
        gesturesEnabled = gesturesEnabled,
        drawerContent = {
            ModalDrawerSheet(
                drawerState = drawerState,
                modifier = Modifier
                    .width(300.dp)
                    .fillMaxHeight(),
                drawerShape = RectangleShape,
                drawerTonalElevation = 0.dp,
                drawerContainerColor = MaterialTheme.colorScheme.surface,
                drawerContentColor = MaterialTheme.colorScheme.onSurface,
            ) {
                AccountDrawerContent(
                    onOpenSettings = onOpenSettings,
                    onOpenProfile = onOpenProfile,
                )
            }
        },
    ) {
        Box(Modifier.fillMaxSize()) {
            content()
            if (gesturesEnabled) {
                Box(
                    modifier = Modifier
                        .align(Alignment.CenterStart)
                        .fillMaxHeight()
                        .width(40.dp)
                        .padding(top = 56.dp)
                        .systemGestureExclusion(),
                )
            }
        }
    }
}

@Composable
internal fun AccountDrawerContent(
    onOpenSettings: () -> Unit,
    onOpenProfile: (handle: String, name: String) -> Unit,
) {
    val me = currentReaderIdentity()
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(bottom = 28.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(role = Role.Button) { onOpenProfile(me.handle, me.name) }
                .padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 12.dp)
                .semantics { contentDescription = "Profile" },
        ) {
            Avatar(name = me.name, size = 64.dp)
            Spacer(Modifier.height(12.dp))
            Text(me.name, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(2.dp))
            Text(
                text = "@${me.handle}",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outline,
            thickness = 0.6.dp,
        )
        DrawerRow(
            icon = Icons.Outlined.Person,
            label = "Profile",
            onClick = { onOpenProfile(me.handle, me.name) },
        )
        DrawerRow(
            icon = Icons.Outlined.Settings,
            label = "Settings",
            onClick = onOpenSettings,
        )
    }
}

@Composable
private fun DrawerRow(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 64.dp)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onBackground,
            modifier = Modifier.size(26.dp),
        )
        Spacer(Modifier.width(16.dp))
        Text(
            text = label,
            style = MaterialTheme.typography.headlineLarge,
        )
    }
}
