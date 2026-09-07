package com.mewo.reader.ui

import androidx.compose.runtime.Composable
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.mewo.reader.data.LibraryRepository
import com.mewo.reader.ui.library.LibraryScreen
import com.mewo.reader.ui.library.LibraryViewModel
import com.mewo.reader.ui.reader.ReaderScreen
import com.mewo.reader.ui.reader.ReaderViewModel

@Composable
fun MewoNav(repository: LibraryRepository) {
    val nav = rememberNavController()
    NavHost(navController = nav, startDestination = "library") {
        composable("library") {
            val vm: LibraryViewModel = viewModel(
                factory = LibraryViewModel.factory(repository),
            )
            LibraryScreen(
                viewModel = vm,
                onOpenBook = { id -> nav.navigate("reader/$id") },
            )
        }
        composable(
            route = "reader/{bookId}",
            arguments = listOf(navArgument("bookId") { type = NavType.StringType }),
        ) { entry ->
            val bookId = entry.arguments?.getString("bookId") ?: return@composable
            val vm: ReaderViewModel = viewModel(
                factory = ReaderViewModel.factory(repository, bookId),
            )
            ReaderScreen(
                viewModel = vm,
                onBack = { nav.popBackStack() },
            )
        }
    }
}
