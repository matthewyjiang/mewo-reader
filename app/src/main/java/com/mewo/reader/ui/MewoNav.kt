package com.mewo.reader.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.mewo.reader.data.LibraryRepository
import com.mewo.reader.ui.components.AppTab
import com.mewo.reader.ui.components.HomeTabBar
import com.mewo.reader.ui.library.LibraryScreen
import com.mewo.reader.ui.library.LibraryViewModel
import com.mewo.reader.ui.likes.LikesScreen
import com.mewo.reader.ui.likes.LikesViewModel
import com.mewo.reader.ui.reader.ReaderScreen
import com.mewo.reader.ui.reader.ReaderViewModel
import com.mewo.reader.ui.search.SearchScreen
import com.mewo.reader.ui.search.SearchViewModel
import com.mewo.reader.ui.theme.DisplaySheet

@Composable
fun MewoNav(repository: LibraryRepository) {
    val nav = rememberNavController()
    var showDisplay by rememberSaveable { mutableStateOf(false) }
    var tab by rememberSaveable { mutableStateOf(AppTab.Home) }

    fun goTab(next: AppTab) {
        tab = next
        val route = when (next) {
            AppTab.Home -> "library"
            AppTab.Search -> "search"
            AppTab.Likes -> "likes"
        }
        nav.navigate(route) {
            popUpTo("library") {
                inclusive = false
                saveState = true
            }
            launchSingleTop = true
            restoreState = true
        }
    }

    fun openBook(id: String, postId: String? = null) {
        val dest = if (postId.isNullOrBlank()) "reader/$id" else "reader/$id?postId=$postId"
        nav.navigate(dest)
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        NavHost(
            navController = nav,
            startDestination = "library",
            modifier = Modifier.weight(1f),
        ) {
            composable("library") {
                val vm: LibraryViewModel = viewModel(
                    factory = LibraryViewModel.factory(repository),
                )
                LibraryScreen(
                    viewModel = vm,
                    onOpenBook = { id -> openBook(id) },
                    onDisplay = { showDisplay = true },
                )
            }
            composable("search") {
                val vm: SearchViewModel = viewModel(
                    factory = SearchViewModel.factory(repository),
                )
                SearchScreen(
                    viewModel = vm,
                    onOpenBook = { id -> openBook(id) },
                    onOpenPost = { bookId, postId -> openBook(bookId, postId) },
                )
            }
            composable("likes") {
                val vm: LikesViewModel = viewModel(
                    factory = LikesViewModel.factory(repository),
                )
                LikesScreen(
                    viewModel = vm,
                    onOpenPost = { bookId, postId -> openBook(bookId, postId) },
                )
            }
            composable(
                route = "reader/{bookId}?postId={postId}",
                arguments = listOf(
                    navArgument("bookId") { type = NavType.StringType },
                    navArgument("postId") {
                        type = NavType.StringType
                        defaultValue = ""
                    },
                ),
            ) { entry ->
                val bookId = entry.arguments?.getString("bookId") ?: return@composable
                val postId = entry.arguments?.getString("postId")?.ifBlank { null }
                val vm: ReaderViewModel = viewModel(
                    factory = ReaderViewModel.factory(repository, bookId, postId),
                )
                ReaderScreen(
                    viewModel = vm,
                    onBack = { nav.popBackStack() },
                    onDisplay = { showDisplay = true },
                )
            }
        }
        HomeTabBar(
            selected = tab,
            onSelect = ::goTab,
        )
    }
    if (showDisplay) {
        DisplaySheet(onDismiss = { showDisplay = false })
    }
}
