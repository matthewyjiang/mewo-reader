package com.mewo.reader.ui.search

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.SearchKind

/**
 * Six static rows for a first load. Same bone color as [com.mewo.reader.ui.catalog.CatalogSkeleton].
 * Shapes follow the web skeleton so the list does not jump when rows land.
 */
@Composable
internal fun SearchSkeleton(kind: SearchKind, rows: Int = 6) {
    if (kind == SearchKind.Catalog) return
    val bone = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.16f)
    Column(
        Modifier
            .fillMaxWidth()
            .semantics { contentDescription = "Loading results…" },
    ) {
        repeat(rows) {
            when (kind) {
                SearchKind.Accounts -> AccountBone(bone)
                SearchKind.Books -> BookBone(bone)
                SearchKind.Posts -> PostBone(bone)
                SearchKind.Catalog -> Unit
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outline, thickness = 0.6.dp)
        }
    }
}

@Composable
private fun AccountBone(bone: Color) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AvatarBone(bone)
        Spacer(Modifier.width(12.dp))
        Column(
            Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Line(bone, width = 0.72f, height = 14)
            Line(bone, width = 0.46f, height = 12)
        }
    }
}

@Composable
private fun BookBone(bone: Color) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 72.dp)
            .padding(start = 16.dp, top = 12.dp, end = 16.dp, bottom = 16.dp),
        verticalAlignment = Alignment.Top,
    ) {
        AvatarBone(bone)
        Spacer(Modifier.width(12.dp))
        Column(
            Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Line(bone, width = 0.72f, height = 14)
            Line(bone, width = 0.46f, height = 12)
            Line(bone, width = 0.28f, height = 12)
        }
    }
}

@Composable
private fun PostBone(bone: Color) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = 16.dp, end = 12.dp, top = 12.dp, bottom = 4.dp),
        verticalAlignment = Alignment.Top,
    ) {
        AvatarBone(bone)
        Spacer(Modifier.width(12.dp))
        Column(
            Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Line(bone, width = 0.72f, height = 14)
            Line(bone, width = 0.88f, height = 12)
            Line(bone, width = 0.46f, height = 12)
        }
    }
}

@Composable
private fun AvatarBone(bone: Color) {
    Box(
        Modifier
            .size(40.dp)
            .clip(CircleShape)
            .background(bone),
    )
}

@Composable
private fun Line(bone: Color, width: Float, height: Int) {
    Box(
        Modifier
            .fillMaxWidth(width)
            .height(height.dp)
            .clip(RoundedCornerShape(4.dp))
            .background(bone),
    )
}
