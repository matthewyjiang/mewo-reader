package com.mewo.reader.ui

import android.net.Uri
import android.os.Build
import android.view.inputmethod.InputMethodManager
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.imePadding
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
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.mewo.reader.ui.components.AccountDrawer
import com.mewo.reader.ui.components.AppTab
import com.mewo.reader.ui.components.HomeTabBar
import com.mewo.reader.ui.components.rememberHideOnScrollState
import com.mewo.reader.ui.library.LibraryScreen
import com.mewo.reader.ui.library.LibraryViewModel
import com.mewo.reader.ui.likes.LikesScreen
import com.mewo.reader.ui.likes.LikesViewModel
import com.mewo.reader.ui.profile.ProfileScreen
import com.mewo.reader.ui.profile.ProfileViewModel
import com.mewo.reader.ui.reader.ReaderScreen
import com.mewo.reader.ui.reader.ReaderViewModel
import com.mewo.reader.ui.search.SearchScreen
import com.mewo.reader.ui.search.SearchViewModel
import com.mewo.reader.ui.settings.SettingsAccountPage
import com.mewo.reader.ui.settings.SettingsDisplayPage
import com.mewo.reader.ui.settings.SettingsHome
import com.mewo.reader.ui.settings.SettingsLibraryPage
import com.mewo.reader.ui.settings.SettingsRoutes
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
fun MewoNav() {
    val store = LocalLibraryStore.current
    val nav = rememberNavController()
    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    var tab by rememberSaveable { mutableStateOf(AppTab.Home) }
    val chrome = rememberHideOnScrollState()
    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route.orEmpty()
    val onReader = route.startsWith("reader")
    val onSettings = SettingsRoutes.matches(route)
    val onProfile = route.startsWith("profile")
    val coverFeed = onReader || onSettings || onProfile
    val activity = LocalActivity.current
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    fun hideIme() {
        focusManager.clearFocus(force = true)
        keyboard?.hide()
        val act = activity ?: return
        act.currentFocus?.clearFocus()
        act.window.decorView.clearFocus()
        act.getSystemService(InputMethodManager::class.java)
            ?.hideSoftInputFromWindow(act.window.decorView.windowToken, 0)
    }
    LaunchedEffect(tab, onReader, onSettings, onProfile) {
        if (tab != AppTab.Search || onReader || onSettings || onProfile) {
            delay(50)
            hideIme()
        }
    }
    val openAccount: () -> Unit = {
        hideIme()
        scope.launch { drawerState.open() }
    }
    LaunchedEffect(route) {
        if (!onReader) chrome.show()
    }

    // The system Back gesture owns the left edge; the drawer cannot.
    // A swipe with progress opens the drawer. A Back button press, which
    // has no progress events, backgrounds the task so the themed
    // launcher trampoline does not finish the reader activity.
    PredictiveBackHandler(
        enabled = !coverFeed && !drawerState.isOpen && activity != null,
    ) { progress ->
        var swipe = false
        try {
            progress.collect { swipe = true }
        } catch (e: CancellationException) {
            throw e
        }
        if (swipe) {
            drawerState.open()
        } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            activity?.moveTaskToBack(true)
        }
    }

    fun goTab(next: AppTab) {
        hideIme()
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
        hideIme()
        chrome.show()
        scope.launch { drawerState.close() }
        val dest = if (postId.isNullOrBlank()) "reader/$id" else "reader/$id?postId=$postId"
        nav.navigate(dest)
    }

    fun openProfile(handle: String, name: String = "") {
        if (handle.isBlank()) return
        hideIme()
        chrome.show()
        scope.launch { drawerState.close() }
        val current = entry?.arguments?.getString("handle")
        if (onProfile && current.equals(handle, ignoreCase = true)) return
        val dest = buildString {
            append("profile/${Uri.encode(handle)}")
            if (name.isNotBlank()) append("?name=${Uri.encode(name)}")
        }
        nav.navigate(dest)
    }

    fun openSettings() {
        hideIme()
        chrome.show()
        scope.launch { drawerState.close() }
        if (!onSettings) {
            nav.navigate(SettingsRoutes.Home) {
                launchSingleTop = true
            }
        }
    }

    AccountDrawer(
        drawerState = drawerState,
        gesturesEnabled = !coverFeed,
        onOpenSettings = ::openSettings,
        onOpenProfile = ::openProfile,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .imePadding(),
        ) {
            NavHost(
                navController = nav,
                startDestination = "library",
                modifier = Modifier.weight(1f),
            ) {
                composable("library") {
                    val vm: LibraryViewModel = viewModel(
                        factory = LibraryViewModel.factory(store),
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
                        factory = SearchViewModel.factory(store),
                    )
                    SearchScreen(
                        viewModel = vm,
                        onOpenBook = { id -> openBook(id) },
                        onOpenPost = { bookId, postId -> openBook(bookId, postId) },
                        onOpenAccount = openAccount,
                        onOpenProfile = ::openProfile,
                        hideOnScroll = chrome,
                    )
                }
                composable("likes") {
                    val vm: LikesViewModel = viewModel(
                        factory = LikesViewModel.factory(store),
                    )
                    LikesScreen(
                        viewModel = vm,
                        onOpenPost = { bookId, postId -> openBook(bookId, postId) },
                        onOpenAccount = openAccount,
                        onOpenProfile = ::openProfile,
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
                        factory = ReaderViewModel.factory(store, bookId, postId),
                    )
                    ReaderScreen(
                        viewModel = vm,
                        onBack = {
                            chrome.show()
                            nav.popBackStack()
                        },
                        onOpenProfile = ::openProfile,
                        hideOnScroll = chrome,
                    )
                }
                composable(
                    route = "profile/{handle}?name={name}",
                    arguments = listOf(
                        navArgument("handle") { type = NavType.StringType },
                        navArgument("name") {
                            type = NavType.StringType
                            defaultValue = ""
                        },
                    ),
                    enterTransition = { settingsEnter() },
                    exitTransition = { settingsExit() },
                    popEnterTransition = { settingsPopEnter() },
                    popExitTransition = { settingsPopExit() },
                ) { dest ->
                    val handle = dest.arguments?.getString("handle") ?: return@composable
                    val name = dest.arguments?.getString("name").orEmpty()
                    val vm: ProfileViewModel = viewModel(
                        factory = ProfileViewModel.factory(store, handle),
                    )
                    ProfileScreen(
                        viewModel = vm,
                        displayName = name,
                        onBack = {
                            chrome.show()
                            nav.popBackStack()
                        },
                        onOpenProfile = ::openProfile,
                        hideOnScroll = chrome,
                    )
                }
                composable(
                    route = SettingsRoutes.Home,
                    enterTransition = { settingsEnter() },
                    exitTransition = { settingsExit() },
                    popEnterTransition = { settingsPopEnter() },
                    popExitTransition = { settingsPopExit() },
                ) {
                    SettingsHome(
                        onBack = {
                            chrome.show()
                            nav.popBackStack()
                        },
                        onOpenAccount = { nav.navigate(SettingsRoutes.Account) },
                        onOpenDisplay = { nav.navigate(SettingsRoutes.Display) },
                        onOpenLibrary = { nav.navigate(SettingsRoutes.Library) },
                    )
                }
                composable(
                    route = SettingsRoutes.Account,
                    enterTransition = { settingsEnter() },
                    exitTransition = { settingsExit() },
                    popEnterTransition = { settingsPopEnter() },
                    popExitTransition = { settingsPopExit() },
                ) {
                    SettingsAccountPage(
                        onBack = { nav.popBackStack() },
                    )
                }
                composable(
                    route = SettingsRoutes.Display,
                    enterTransition = { settingsEnter() },
                    exitTransition = { settingsExit() },
                    popEnterTransition = { settingsPopEnter() },
                    popExitTransition = { settingsPopExit() },
                ) {
                    SettingsDisplayPage(
                        onBack = { nav.popBackStack() },
                    )
                }
                composable(
                    route = SettingsRoutes.Library,
                    enterTransition = { settingsEnter() },
                    exitTransition = { settingsExit() },
                    popEnterTransition = { settingsPopEnter() },
                    popExitTransition = { settingsPopExit() },
                ) {
                    SettingsLibraryPage(
                        onBack = { nav.popBackStack() },
                    )
                }
            }
            HomeTabBar(
                selected = tab,
                onSelect = ::goTab,
                visible = chrome.visible && !onSettings,
            )
        }
    }
}

private const val SettingsMotionMs = 220

private fun AnimatedContentTransitionScope<*>.settingsEnter() =
    fadeIn(tween(SettingsMotionMs, easing = FastOutSlowInEasing)) +
        slideIntoContainer(
            towards = AnimatedContentTransitionScope.SlideDirection.Left,
            animationSpec = tween(SettingsMotionMs, easing = FastOutSlowInEasing),
        )

private fun AnimatedContentTransitionScope<*>.settingsExit() =
    fadeOut(tween(SettingsMotionMs, easing = FastOutSlowInEasing)) +
        slideOutOfContainer(
            towards = AnimatedContentTransitionScope.SlideDirection.Left,
            animationSpec = tween(SettingsMotionMs, easing = FastOutSlowInEasing),
        )

private fun AnimatedContentTransitionScope<*>.settingsPopEnter() =
    fadeIn(tween(SettingsMotionMs, easing = FastOutSlowInEasing)) +
        slideIntoContainer(
            towards = AnimatedContentTransitionScope.SlideDirection.Right,
            animationSpec = tween(SettingsMotionMs, easing = FastOutSlowInEasing),
        )

private fun AnimatedContentTransitionScope<*>.settingsPopExit() =
    fadeOut(tween(SettingsMotionMs, easing = FastOutSlowInEasing)) +
        slideOutOfContainer(
            towards = AnimatedContentTransitionScope.SlideDirection.Right,
            animationSpec = tween(SettingsMotionMs, easing = FastOutSlowInEasing),
        )
