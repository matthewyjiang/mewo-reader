package com.mewo.reader.ui.catalog

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.mewo.reader.data.CatalogFilter
import com.mewo.reader.data.CatalogFilters

enum class CatalogFilterMenu { Genre, Sort }

/**
 * Genre and sort menus, the first row of the catalog list. The menu opens
 * below the buttons in this layout, not in a popup window, so a screenshot
 * of the screen includes it (the same choice as [com.mewo.reader.ui.library.AddBookAnchor]).
 * Picking the first sort stores null, so the default is All genres + null.
 */
@Composable
fun CatalogFilterRow(
    filters: CatalogFilters,
    filter: CatalogFilter,
    searching: Boolean,
    onChange: (CatalogFilter) -> Unit,
    modifier: Modifier = Modifier,
    startOpen: CatalogFilterMenu? = null,
) {
    var open by remember { mutableStateOf(startOpen) }
    var sortStart by remember { mutableIntStateOf(0) }
    BackHandler(enabled = open != null) { open = null }
    val firstSort = filters.sorts.firstOrNull()?.id
    val genreName = filters.genres.firstOrNull { it.id == filter.genre }?.name ?: ALL_GENRES
    val sortName = filters.sorts.firstOrNull { it.id == (filter.sort ?: firstSort) }?.name.orEmpty()
    val showSort = !searching && filters.sorts.isNotEmpty()
    Column(modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            FilterButton(
                label = genreName,
                tag = "catalog-genre",
                modifier = Modifier.weight(1f, fill = false),
                onClick = { open = if (open == CatalogFilterMenu.Genre) null else CatalogFilterMenu.Genre },
            )
            if (showSort) {
                FilterButton(
                    label = sortName,
                    tag = "catalog-sort",
                    modifier = Modifier.onGloballyPositioned { sortStart = it.positionInParent().x.toInt() },
                    onClick = { open = if (open == CatalogFilterMenu.Sort) null else CatalogFilterMenu.Sort },
                )
            }
        }
        when (open) {
            CatalogFilterMenu.Genre -> FilterMenu(
                items = listOf<Pair<String?, String>>(null to ALL_GENRES) + filters.genres.map { it.id to it.name },
                selected = filter.genre,
                start = 0,
                onPick = { id ->
                    open = null
                    onChange(filter.copy(genre = id))
                },
            )
            CatalogFilterMenu.Sort -> if (showSort) {
                FilterMenu(
                    items = filters.sorts.map { it.id to it.name },
                    selected = filter.sort ?: firstSort,
                    start = sortStart,
                    onPick = { id ->
                        open = null
                        onChange(filter.copy(sort = id.takeIf { it != firstSort }))
                    },
                )
            }
            null -> Unit
        }
    }
}

private const val ALL_GENRES = "All genres"

@Composable
private fun FilterButton(
    label: String,
    tag: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scheme = MaterialTheme.colorScheme
    OutlinedButton(
        onClick = onClick,
        modifier = modifier
            .height(36.dp)
            .testTag(tag),
        shape = CircleShape,
        border = BorderStroke(1.dp, scheme.outline),
        contentPadding = PaddingValues(start = 16.dp, end = 12.dp),
        colors = ButtonDefaults.outlinedButtonColors(contentColor = scheme.onBackground),
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.titleSmall,
            color = scheme.onBackground,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f, fill = false),
        )
        Spacer(Modifier.width(4.dp))
        Icon(
            imageVector = Icons.Outlined.KeyboardArrowDown,
            contentDescription = null,
            modifier = Modifier.size(18.dp),
        )
    }
}

@Composable
private fun FilterMenu(
    items: List<Pair<String?, String>>,
    selected: String?,
    start: Int,
    onPick: (String?) -> Unit,
) {
    val startDp = with(LocalDensity.current) { start.toDp() }
    Surface(
        modifier = Modifier.padding(start = startDp + 16.dp, end = 16.dp, bottom = 8.dp),
        shape = RoundedCornerShape(4.dp),
        color = MaterialTheme.colorScheme.surface,
        shadowElevation = 3.dp,
    ) {
        Column(Modifier.widthIn(min = 200.dp, max = 280.dp)) {
            items.forEach { (id, name) ->
                val isSelected = id == selected
                DropdownMenuItem(
                    text = { Text(name) },
                    trailingIcon = if (isSelected) {
                        { Icon(Icons.Outlined.Check, contentDescription = null) }
                    } else {
                        null
                    },
                    onClick = { onPick(id) },
                    modifier = Modifier.semantics { this.selected = isSelected },
                )
            }
        }
    }
}
