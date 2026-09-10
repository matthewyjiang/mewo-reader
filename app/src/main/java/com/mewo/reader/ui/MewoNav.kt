package com.mewo.reader.ui

import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.mewo.reader.data.LibraryRepository
import com.mewo.reader.ui.components.AccountDrawer
import com.mewo.reader.ui.components.AppTab
import com.mewo.reader.ui.components.HomeTabBar
import com.mewo.reader.ui.components.rememberHideOnScrollState
import com.mewo.reader.ui.library.LibraryScreen
import com.mewo.reader.ui.library.LibraryViewModel
import com.mewo.reader.ui.likes.LikesScreen
import com.mewo.reader.ui.likes.LikesViewModel
import com.mewo.reader.ui.reader.ReaderScreen
import com.mewo.reader.ui.reader.ReaderViewModel
import com.mewo.reader.ui.search.SearchScreen
import com.mewo.reader.ui.search.SearchViewModel
import kotlinx.coroutines.launch

@Composable
fun MewoNav(repository: LibraryRepository) {
    val nav = rememberNavController()
    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    var tab by rememberSaveable { mutableStateOf(AppTab.Home) }
    val chrome = rememberHideOnScrollState()
    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route.orEmpty()
    val onReader = route.startsWith("reader")
    val activity = LocalActivity.current
    val openAccount: () -> Unit = { scope.launch { drawerState.open() } }
    LaunchedEffect(route) {
        if (!onReader) chrome.show()
    }

    // A themed launcher entry starts the reader, rather than the home process.
    // Preserve Android 12+ root-Back backgrounding without intercepting the
    // drawer or the reader's own navigation back to the library.
    BackHandler(
        enabled = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
            entry?.destination?.route == "library" &&
            !drawerState.isOpen &&
            activity != null,
    ) {
        activity?.moveTaskToBack(true)
    }

    fun goTab(next: AppTab) {
        chrome.show()
        tab = next
        val dest = when (next) {
            AppTab.Home -> "library"
            AppTab.Search -> "search"
            AppTab.Likes -> "likes"
        }
        nav.navigate(dest) {
            popUpTo("library") {
                inclusive = false
                saveState = true
            }
            launchSingleTop = true
            restoreState = true
        }
    }

    fun openBook(id: String, postId: String? = null) {
        chrome.show()
        scope.launch { drawerState.close() }
        val dest = if (postId.isNullOrBlank()) "reader/$id" else "reader/$id?postId=$postId"
        nav.navigate(dest)
    }

    AccountDrawer(
        drawerState = drawerState,
        gesturesEnabled = !onReader,
    ) {
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
                        onOpenAccount = openAccount,
                        hideOnScroll = chrome,
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
                        onOpenAccount = openAccount,
                        hideOnScroll = chrome,
                    )
                }
                composable("likes") {
                    val vm: LikesViewModel = viewModel(
                        factory = LikesViewModel.factory(repository),
                    )
                    LikesScreen(
                        viewModel = vm,
                        onOpenPost = { bookId, postId -> openBook(bookId, postId) },
                        onOpenAccount = openAccount,
                        hideOnScroll = chrome,
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
                ) { dest ->
                    val bookId = dest.arguments?.getString("bookId") ?: return@composable
                    val postId = dest.arguments?.getString("postId")?.ifBlank { null }
                    val vm: ReaderViewModel = viewModel(
                        factory = ReaderViewModel.factory(repository, bookId, postId),
                    )
                    ReaderScreen(
                        viewModel = vm,
                        onBack = {
                            chrome.show()
                            nav.popBackStack()
                        },
                        hideOnScroll = chrome,
                    )
                }
            }
            HomeTabBar(
                selected = tab,
                onSelect = ::goTab,
                visible = chrome.visible,
            )
        }
    }
}
