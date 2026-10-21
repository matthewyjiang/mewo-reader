package com.mewo.reader.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.unit.dp
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import com.mewo.reader.data.AccountHit
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.BookRequest
import com.mewo.reader.data.CatalogWork
import com.mewo.reader.data.Rating
import com.mewo.reader.data.RequestDetail
import com.mewo.reader.data.ServerCopy
import com.mewo.reader.data.WorkDetail
import com.mewo.reader.data.WorkInfo
import com.mewo.reader.data.WorkStatus
import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.hasReposted
import com.mewo.reader.data.repostCount
import com.mewo.reader.data.HostedSession
import com.mewo.reader.data.LocalProfile
import com.mewo.reader.data.PostKind
import com.mewo.reader.data.ProfileRecord
import com.mewo.reader.ui.catalog.CatalogEmpty
import com.mewo.reader.ui.catalog.CatalogNotice
import com.mewo.reader.ui.catalog.CatalogRow
import com.mewo.reader.ui.catalog.CatalogSheetContent
import com.mewo.reader.ui.catalog.CatalogSheetState
import com.mewo.reader.ui.catalog.CatalogSkeleton
import com.mewo.reader.ui.components.AccountDrawer
import com.mewo.reader.ui.components.ErrorStatus
import com.mewo.reader.ui.components.AppTab
import com.mewo.reader.ui.components.BrandMark
import com.mewo.reader.ui.components.HomeTabBar
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileButton
import com.mewo.reader.ui.components.UnderlineTabs
import com.mewo.reader.ui.feed.ActivityLine
import com.mewo.reader.ui.profile.ProfileBar
import com.mewo.reader.ui.profile.ProfileHeader
import com.mewo.reader.ui.reader.CommentItem
import com.mewo.reader.ui.reader.TimelinePost
import com.mewo.reader.ui.reader.relativeTime
import com.mewo.reader.ui.search.AccountHitRow
import com.mewo.reader.ui.search.SearchField
import com.mewo.reader.ui.library.AddBookAnchor
import com.mewo.reader.ui.library.BookPost
import com.mewo.reader.ui.library.addBookOptions
import com.mewo.reader.ui.library.EmptyTimeline
import com.mewo.reader.ui.library.LibraryLoading
import com.mewo.reader.ui.library.LibraryNotice
import com.mewo.reader.ui.library.RemoveBookDialog
import com.mewo.reader.ui.library.SectionLabel
import com.mewo.reader.ui.likes.EmptyLikes
import com.mewo.reader.ui.onboarding.OnboardingScreen
import com.mewo.reader.ui.profile.EmptyPosts
import com.mewo.reader.ui.search.SearchEmpty
import com.mewo.reader.ui.search.searchTabs
import com.mewo.reader.ui.settings.AppBuild
import com.mewo.reader.ui.settings.ServerVersionState
import com.mewo.reader.ui.settings.SettingsAboutPage
import com.mewo.reader.ui.settings.SettingsAccountPage
import com.mewo.reader.ui.settings.SettingsDisplayPage
import com.mewo.reader.ui.settings.SettingsHome
import com.mewo.reader.ui.settings.SettingsLibraryPage
import com.mewo.reader.ui.theme.DisplayTheme
import com.mewo.reader.ui.theme.LocalMewoColors
import com.mewo.reader.ui.theme.LocalMewoMode
import com.mewo.reader.ui.theme.LocalReaderFont
import com.mewo.reader.ui.theme.LocalThemeController
import com.mewo.reader.ui.theme.MewoModeController
import com.mewo.reader.ui.theme.ReaderFontController
import com.mewo.reader.ui.theme.ReaderTypeScale
import com.mewo.reader.ui.theme.ThemeController
import com.mewo.reader.ui.theme.mewoTypography
import com.mewo.reader.ui.theme.toColorScheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

/**
 * Layout goldens for the surfaces an agent can check without an emulator.
 * Phone is a Pixel 5. Narrow is the same height at 320dp wide.
 */
@OptIn(ExperimentalMaterial3Api::class)
class SurfaceScreenshots {
    @get:Rule
    val paparazzi = Paparazzi(
        deviceConfig = phone,
        theme = "android:Theme.Material.NoActionBar",
    )

    @Test
    fun hostedLibrary() = shoot("library") { HostedLibrary() }

    @Test
    fun libraryNotice() = shoot("library-notice", displayThemes = DisplayTheme.entries.map { it.id to it }) {
        Column(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f)) {
                HostedLibrary()
                LibraryNotice(
                    message = "Slouching Towards Bethlehem is private. Use Share on Home when you want others to read it.",
                    onDismiss = {},
                    modifier = Modifier.align(Alignment.BottomCenter).padding(16.dp),
                )
            }
            HomeTabBar(selected = AppTab.Home, onSelect = {})
        }
    }

    @Test
    fun settings() = shoot("settings") {
        SettingsHome(
            onBack = {},
            onOpenAccount = {},
            onOpenDisplay = {},
            onOpenLibrary = {},
            onOpenAbout = {},
        )
    }

    @Test
    fun about() = shoot("about") {
        SettingsAboutPage(
            onBack = {},
            app = AppBuild(versionName = "0.1.1", versionCode = 1, commit = "abc1234"),
            server = ServerVersionState.Ready(version = "0.1.2", commit = "def5678"),
        )
    }

    @Test
    fun onboarding() = shoot("onboarding") { OnboardingScreen() }

    @Test
    fun signIn() = shoot("sign-in", signedIn = false) {
        SettingsLibraryPage(onBack = {}, signupsOpen = true)
    }

    @Test
    fun signInSignupsOff() = shoot("sign-in-closed", signedIn = false) {
        SettingsLibraryPage(onBack = {}, signupsOpen = false)
    }

    @Test
    fun createAccount() = shoot("create-account", signedIn = false) {
        SettingsLibraryPage(onBack = {}, startOnCreate = true, signupsOpen = true)
    }

    @Test
    fun libraryAccount() = shoot("library-account") {
        SettingsLibraryPage(onBack = {})
    }

    @Test
    fun display() = shoot("display") {
        SettingsDisplayPage(onBack = {})
    }

    @Test
    fun account() = shoot("account") {
        SettingsAccountPage(onBack = {})
    }

    @Test
    fun drawer() = shoot("drawer") {
        AccountDrawer(
            drawerState = rememberDrawerState(DrawerValue.Open),
            gesturesEnabled = false,
            onOpenSettings = {},
            onOpenProfile = { _, _ -> },
        ) {
            HomeBooks()
        }
    }

    @Test
    fun libraryLoading() = shoot("library-loading") {
        Surface { LibraryLoading() }
    }

    @Test
    fun emptyLibrary() = shoot("empty-library") {
        Surface {
            EmptyTimeline(onAdd = {}, onSample = {})
        }
    }

    @Test
    fun removeBook() = shoot("remove-book") {
        RemoveBookDialog(
            book = privateBook,
            hosted = true,
            onConfirm = {},
            onDismiss = {},
        )
    }

    @Test
    fun likesEmpty() = shoot("likes-empty") { Surface { EmptyLikes() } }

    @Test
    fun searchEmpty() = shoot("search-empty") { Surface { SearchEmpty() } }

    @Test
    fun postsEmpty() = shoot("posts-empty") { Surface { EmptyPosts() } }

    @Test
    fun searchAccounts() = shoot("search-accounts") {
        SearchChrome("accounts") {
            accountsMatching(SearchQuery, accountDirectory).forEach { account ->
                AccountHitRow(account) {}
            }
        }
    }

    @Test
    fun searchBooks() = shoot("search-books") {
        SearchChrome("books") {
            shelfBook(sharedBook)
        }
    }

    @Test
    fun searchPosts() = shoot("search-posts") {
        SearchChrome("posts") {
            passage(reposted = false, count = 0)
        }
    }

    @Test
    fun profilePosts() = shoot("profile-posts") {
        ProfileChrome("posts") {
            commentItem(reposted = true, repostCount = 2)
        }
    }

    @Test
    fun profileReposts() = shoot("profile-reposts") {
        ProfileChrome("reposts") {
            passage(reposted = true, count = 1)
        }
    }

    @Test
    fun profileBooks() = shoot("profile-books") {
        ProfileChrome("books") {
            shelfBook(sharedBook)
        }
    }

    @Test
    fun homeFeed() = shoot("home-feed") {
        val line = FeedPost("0", PostKind.Body, "Emma Woodhouse, handsome, clever, and rich.", "One")
        // One index for the book, the same object both rows read.
        val index = CommentIndex(
            counts = mapOf(line.id to 1),
            mine = setOf(line.id),
            reposts = mapOf(line.id to 1),
            reposted = emptySet(),
        )
        HomeChrome("feed") {
            ActivityLine(username = "byron") {}
            passage(line, index)
            commentItem(line, index)
        }
    }

    @Test
    fun homeBooks() = shoot("home-books") { HomeBooks() }

    @Test
    fun repostActive() = shoot("repost-active") {
        SearchChrome("posts") {
            passage(reposted = true, count = 2)
        }
    }

    @Test
    fun catalogTrending() = shoot("catalog-trending") {
        SearchChrome("catalog", query = "", tabs = searchTabs(hosted = true, canRequest = true)) {
            CatalogHeadingShot()
            CatalogRow(work = duneWork, request = null, onOpen = {}, onAct = {})
            CatalogRow(work = emmaWork, request = null, onOpen = {}, onAct = {})
            CatalogRow(work = longWork, request = longWork.request, onOpen = {}, onAct = {})
            CatalogRow(work = failedWork, request = failedWork.request, onOpen = {}, onAct = {})
            CatalogRow(work = canceledWork, request = canceledWork.request, onOpen = {}, onAct = {})
            CatalogRow(work = shelfWork, request = shelfWork.request, onOpen = {}, onAct = {})
            CatalogRow(work = importedWork, request = importedWork.request, onOpen = {}, onAct = {})
        }
    }

    @Test
    fun catalogShortQuery() = shoot("catalog-short-query") {
        SearchChrome("catalog", query = "du", tabs = searchTabs(hosted = true, canRequest = true)) {
            CatalogHeadingShot()
            CatalogRow(work = duneWork, request = null, onOpen = {}, onAct = {})
        }
    }

    @Test
    fun catalogLoading() = shoot("catalog-loading") {
        SearchChrome("catalog", query = "", tabs = searchTabs(hosted = true, canRequest = true)) {
            CatalogHeadingShot()
            CatalogSkeleton()
        }
    }

    @Test
    fun catalogEmpty() = shoot("catalog-empty") {
        SearchChrome("catalog", query = "", tabs = searchTabs(hosted = true, canRequest = true)) {
            CatalogEmpty("")
        }
    }

    @Test
    fun catalogError() = shoot("catalog-error") {
        SearchChrome("catalog", query = "", tabs = searchTabs(hosted = true, canRequest = true)) {
            ErrorStatus(
                message = "Couldn't reach the server. Check your connection.",
                onRetry = {},
            )
        }
    }

    @Test
    fun catalogNotice() = shoot("catalog-notice") {
        val focus = remember { FocusRequester() }
        SideEffect { focus.requestFocus() }
        Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            MewoAppBar(
                leading = { ProfileButton(onClick = {}) },
                content = {
                    SearchField(
                        query = "",
                        onQueryChange = {},
                        hosted = true,
                        focusRequester = focus,
                        modifier = Modifier.weight(1f).padding(end = 8.dp),
                    )
                },
            )
            UnderlineTabs(
                tabs = searchTabs(hosted = true, canRequest = true),
                selected = "catalog",
                onSelect = {},
            )
            Box(Modifier.weight(1f).fillMaxWidth()) {
                Column {
                    CatalogHeadingShot()
                    CatalogRow(work = duneWork, request = null, onOpen = {}, onAct = {})
                }
                CatalogNotice(
                    message = "Could not request that book.",
                    onDismiss = {},
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .padding(start = 16.dp, end = 16.dp, bottom = 16.dp)
                        .fillMaxWidth(),
                )
            }
            HomeTabBar(selected = AppTab.Search, onSelect = {})
        }
    }

    @Test
    fun catalogSheetRequest() = shoot("catalog-sheet-request") {
        catalogSheet(duneSheet(WorkStatus(active = 1, cap = 5, otherRequests = otherRequests)))
    }

    @Test
    fun catalogSheetDownloading() = shoot("catalog-sheet-downloading") {
        catalogSheet(
            duneSheet(
                WorkStatus(
                    request = detailRequest(state = "downloading", phase = "downloading", progress = 42),
                    cap = 5,
                ),
            ),
        )
    }

    @Test
    fun catalogSheetFailed() = shoot("catalog-sheet-failed") {
        catalogSheet(
            duneSheet(
                WorkStatus(
                    request = detailRequest(
                        state = "failed",
                        phase = "failed",
                        error = "No release found.",
                    ),
                    cap = 5,
                ),
            ),
        )
    }

    @Test
    fun catalogSheetOpen() = shoot("catalog-sheet-open") {
        catalogSheet(
            duneSheet(WorkStatus(onServer = ServerCopy(bookId = "b1", mine = true), cap = 5)),
        )
    }

    @Test
    fun catalogSheetAgain() = shoot("catalog-sheet-again") {
        catalogSheet(
            duneSheet(
                WorkStatus(
                    request = detailRequest(state = "imported", phase = "imported"),
                    cap = 5,
                ),
            ),
        )
    }

    @Test
    fun catalogSheetCanceled() = shoot("catalog-sheet-canceled") {
        catalogSheet(
            duneSheet(
                WorkStatus(
                    request = detailRequest(state = "canceled", phase = "canceled"),
                    cap = 5,
                ),
            ),
        )
    }

    @Test
    fun addBookMenu() = shoot("add-book-menu") {
        Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            MewoAppBar(leading = { ProfileButton(onClick = {}) }, center = { BrandMark() })
            Box(Modifier.weight(1f).fillMaxWidth()) {
                Column {
                    SectionLabel("My books")
                    shelfBook(privateBook)
                }
                AddBookAnchor(
                    options = addBookOptions(canRequest = true),
                    onUpload = {},
                    onBrowseCatalog = {},
                    menuAlignment = Alignment.End,
                    startExpanded = true,
                    modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp),
                ) { open ->
                    FloatingActionButton(
                        onClick = open,
                        containerColor = MaterialTheme.colorScheme.primary,
                        contentColor = MaterialTheme.colorScheme.onPrimary,
                        shape = CircleShape,
                    ) {
                        Icon(Icons.Outlined.Add, contentDescription = "Add a book")
                    }
                }
            }
            HomeTabBar(selected = AppTab.Home, onSelect = {})
        }
    }

    private fun shoot(
        name: String,
        signedIn: Boolean = true,
        displayThemes: List<Pair<String, DisplayTheme>> = themes,
        content: @Composable () -> Unit,
    ) {
        val profile = if (name == "onboarding") {
            LocalProfile()
        } else {
            LocalProfile(name = "Ada Lovelace", handle = "ada")
        }
        for ((deviceName, device) in devices) {
            paparazzi.unsafeUpdateConfig(deviceConfig = device)
            for ((themeName, theme) in displayThemes) {
                paparazzi.snapshot(name = "${name}_${deviceName}_$themeName") {
                    Frame(
                        theme = theme,
                        profile = profile,
                        signedIn = signedIn,
                        content = content,
                    )
                }
            }
        }
    }
}

@Composable
private fun CatalogHeadingShot() {
    com.mewo.reader.ui.catalog.CatalogHeading("Trending on Open Library")
}

@Composable
private fun catalogSheet(state: CatalogSheetState) {
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        CatalogSheetContent(
            state = state,
            onRequest = {},
            onCancel = {},
            onRetry = {},
            onOpen = {},
            onReload = {},
            onToggleDescription = {},
            onDismiss = {},
            modifier = Modifier.fillMaxSize(),
        )
    }
}

private fun duneSheet(status: WorkStatus) = CatalogSheetState(
    work = duneWork,
    detail = WorkDetail(
        workKey = duneWork.workKey,
        info = duneInfo,
        status = status,
    ),
    loading = false,
)

private fun detailRequest(
    state: String,
    phase: String,
    progress: Int = 0,
    error: String? = null,
) = RequestDetail(
    id = "r1",
    workKey = duneWork.workKey,
    title = duneWork.title,
    author = duneWork.author,
    year = duneWork.year,
    state = state,
    progress = progress,
    error = error,
    phase = phase,
)

private val duneWork = CatalogWork(
    workKey = "/works/OL893415W",
    title = "Dune",
    author = "Frank Herbert",
    year = 1965,
)

private val emmaWork = CatalogWork(
    workKey = "/works/OL53924W",
    title = "Emma",
    author = "Jane Austen",
    year = 1815,
    onServer = ServerCopy(bookId = "emma", ownerUsername = "austen", shared = true),
)

private val failedWork = CatalogWork(
    workKey = "/works/OL2W",
    title = "Frankenstein",
    author = "Mary Shelley",
    year = 1818,
    request = BookRequest(
        id = "r-fail",
        workKey = "/works/OL2W",
        title = "Frankenstein",
        author = "Mary Shelley",
        year = 1818,
        state = "failed",
        error = "No release found.",
    ),
)

private val canceledWork = CatalogWork(
    workKey = "/works/OL3W",
    title = "Persuasion",
    author = "Jane Austen",
    year = 1817,
    request = BookRequest(
        id = "r-cancel",
        workKey = "/works/OL3W",
        title = "Persuasion",
        author = "Jane Austen",
        year = 1817,
        state = "canceled",
    ),
)

private val shelfWork = CatalogWork(
    workKey = "/works/OL4W",
    title = "Pride and Prejudice",
    author = "Jane Austen",
    year = 1813,
    request = BookRequest(
        id = "r-shelf",
        workKey = "/works/OL4W",
        title = "Pride and Prejudice",
        author = "Jane Austen",
        year = 1813,
        state = "imported",
        bookId = "b-pnp",
    ),
)

private val importedWork = CatalogWork(
    workKey = "/works/OL5W",
    title = "The Left Hand of Darkness",
    author = "Ursula K. Le Guin",
    year = 1969,
    request = BookRequest(
        id = "r-again",
        workKey = "/works/OL5W",
        title = "The Left Hand of Darkness",
        author = "Ursula K. Le Guin",
        year = 1969,
        state = "imported",
    ),
)

private val longWork = CatalogWork(
    workKey = "/works/OL1W",
    title = "The Particular Sadness of Lemon Cake and Other Stories from a Very Long Shelf",
    author = "Aimee Bender",
    year = 2010,
    request = BookRequest(
        id = "r-long",
        workKey = "/works/OL1W",
        title = "The Particular Sadness of Lemon Cake and Other Stories from a Very Long Shelf",
        author = "Aimee Bender",
        year = 2010,
        state = "downloading",
        progress = 40,
    ),
)

private val otherRequests = listOf(
    BookRequest(id = "r2", workKey = duneWork.workKey, title = "Dune", state = "requested", requester = "bea"),
    BookRequest(id = "r3", workKey = duneWork.workKey, title = "Dune", state = "requested", requester = "ada"),
)

private val duneInfo = WorkInfo(
    canonicalKey = "/works/OL893415W",
    title = "Dune",
    authors = listOf("Frank Herbert"),
    year = 1965,
    description = "Set on the desert planet Arrakis, Dune follows Paul Atreides as his family takes control of the only source of the spice melange. The story follows the politics of the great houses, the ecology of the desert, and the people who live in it. Paul learns what the spice costs the planet, and what it will cost him to survive it.",
    subjects = listOf("Science fiction", "Desert", "Politics", "Ecology", "Arrakis"),
    awards = listOf("Hugo Award"),
    series = listOf("Dune"),
    pagesMedian = 412,
    hasAcceptedEdition = true,
    rating = Rating(average = 4.2, count = 1000),
)

@Composable
private fun Surface(content: @Composable () -> Unit) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        content()
    }
}

private val phone = DeviceConfig.PIXEL_5.copy(softButtons = false)

/** 320dp wide at the Pixel 5 density (440dpi). */
private val narrow = phone.copy(screenWidth = 880)

private val devices = listOf(
    "phone" to phone,
    "narrow" to narrow,
)

private val themes = listOf(
    "light" to DisplayTheme.XLight,
    "dark" to DisplayTheme.XDark,
)

@Composable
private fun Frame(
    theme: DisplayTheme,
    profile: LocalProfile,
    signedIn: Boolean,
    content: @Composable () -> Unit,
) {
    val palette = theme.palette
    CompositionLocalProvider(
        LocalMewoColors provides palette,
        LocalThemeController provides ThemeController(current = theme, setTheme = {}),
        LocalMewoMode provides MewoModeController(enabled = false, setEnabled = {}),
        LocalReaderFont provides ReaderFontController(
            scale = ReaderTypeScale.Default,
            setStep = {},
        ),
        LocalProfile provides LocalProfileController(profile = profile, save = { _, _ -> }),
        LocalBackend provides BackendController(kind = BackendKind.Hosted, setKind = {}),
        LocalHostedAuth provides HostedAuthController(
            session = if (signedIn) {
                HostedSession(
                    baseUrl = "https://mewo.example",
                    username = "ada",
                    token = "token",
                )
            } else {
                HostedSession(baseUrl = "https://mewo.example")
            },
            setBaseUrl = {},
            signIn = { _, _ -> },
            createAccount = { _, _ -> },
            signOut = {},
        ),
    ) {
        MaterialTheme(
            colorScheme = palette.toColorScheme(),
            typography = mewoTypography(palette.ink),
            content = content,
        )
    }
}

@Composable
private fun HostedLibrary() {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        MewoAppBar(
            leading = { ProfileButton(onClick = {}) },
            center = { BrandMark() },
        )
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState()),
        ) {
            SectionLabel("My books")
            shelfBook(privateBook)
            shelfBook(sharedBook)
            shelfBook(longTitleBook)
            SectionLabel("Shared with me")
            shelfBook(sharedWithMe)
        }
    }
}

@Composable
private fun SearchChrome(
    tab: String,
    query: String = SearchQuery,
    tabs: List<Pair<String, String>> = listOf(
        "accounts" to "Accounts",
        "books" to "Books",
        "posts" to "Posts",
    ),
    content: @Composable () -> Unit,
) {
    val focus = remember { FocusRequester() }
    // The shot is a typed query, so the field is focused the way the web shot is.
    SideEffect { focus.requestFocus() }
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        MewoAppBar(
            leading = { ProfileButton(onClick = {}) },
            content = {
                SearchField(
                    query = query,
                    onQueryChange = {},
                    hosted = true,
                    focusRequester = focus,
                    modifier = Modifier
                        .weight(1f)
                        .padding(end = 8.dp),
                )
            },
        )
        UnderlineTabs(
            tabs = tabs,
            selected = tab,
            onSelect = {},
        )
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            content()
        }
        HomeTabBar(selected = AppTab.Search, onSelect = {})
    }
}

@Composable
private fun ProfileChrome(tab: String, content: @Composable () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        ProfileBar(name = "ada", posts = 2, visible = true, onBack = {})
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            ProfileHeader(
                name = "ada",
                handle = "ada",
                record = ProfileRecord(
                    username = "ada",
                    joinedAt = 1609459200000,
                    postCount = 2,
                    repostCount = 1,
                    mine = true,
                ),
            )
            UnderlineTabs(
                tabs = listOf("posts" to "Posts", "reposts" to "Reposts", "books" to "Books"),
                selected = tab,
                onSelect = {},
            )
            content()
        }
        HomeTabBar(selected = AppTab.Home, onSelect = {})
    }
}

@Composable
private fun HomeChrome(tab: String, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        MewoAppBar(leading = { ProfileButton(onClick = {}) }, center = { BrandMark() })
        UnderlineTabs(
            tabs = listOf("feed" to "Feed", "books" to "Books"),
            selected = tab,
            onSelect = {},
        )
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            content()
        }
        HomeTabBar(selected = AppTab.Home, onSelect = {})
    }
}

@Composable
private fun HomeBooks() {
    HomeChrome("books") {
        SectionLabel("My books")
        shelfBook(privateBook)
        shelfBook(sharedBook)
    }
}

@Composable
private fun commentItem(post: FeedPost, index: CommentIndex) {
    CommentItem(
        author = "Austen",
        handle = "austen",
        post = post,
        cover = null,
        replyName = "ada",
        replyHandle = "ada",
        replyMeta = relativeTime(COMMENT_AT, SHOT_NOW),
        replyText = "This opening still lands.",
        liked = false,
        commentCount = index.count(post.id),
        commented = index.commented(post.id),
        reposted = index.hasReposted(post.id),
        repostCount = index.repostCount(post.id),
        onOpen = {},
        onOpenReply = {},
        onOpenReplyProfile = { _, _ -> },
        onComment = {},
        onLike = {},
        onRepost = {},
        onQuote = {},
        onShare = {},
    )
}

@Composable
private fun commentItem(reposted: Boolean, repostCount: Int) {
    CommentItem(
        author = "Austen",
        handle = "austen",
        post = FeedPost("0", PostKind.Body, "Emma Woodhouse, handsome, clever, and rich.", "One"),
        cover = null,
        replyName = "ada",
        replyHandle = "ada",
        replyMeta = relativeTime(COMMENT_AT, SHOT_NOW),
        replyText = "This opening still lands.",
        liked = false,
        commentCount = 1,
        commented = true,
        reposted = reposted,
        repostCount = repostCount,
        onOpen = {},
        onOpenReply = {},
        onOpenReplyProfile = { _, _ -> },
        onComment = {},
        onLike = {},
        onRepost = {},
        onQuote = {},
        onShare = {},
    )
}

@Composable
private fun passage(post: FeedPost, index: CommentIndex) {
    TimelinePost(
        post = post,
        author = "Austen",
        handle = "austen",
        cover = null,
        liked = false,
        commentCount = index.count(post.id),
        commented = index.commented(post.id),
        reposted = index.hasReposted(post.id),
        repostCount = index.repostCount(post.id),
        onComment = {},
        onLike = {},
        onRepost = {},
        onQuote = {},
        onShare = {},
    )
}

@Composable
private fun passage(reposted: Boolean, count: Int) {
    TimelinePost(
        post = FeedPost("0", PostKind.Body, "Emma Woodhouse, handsome, clever, and rich.", "One"),
        author = "Austen",
        handle = "austen",
        cover = null,
        liked = false,
        commentCount = 1,
        commented = true,
        reposted = reposted,
        repostCount = count,
        onComment = {},
        onLike = {},
        onRepost = {},
        onQuote = {},
        onShare = {},
    )
}

@Composable
private fun shelfBook(book: BookRecord) {
    val mine = book.mine
    BookPost(
        book = book,
        cover = null,
        showOwner = !mine,
        onOpen = {},
        onDelete = if (mine) ({ }) else null,
        onShare = if (mine) ({ _ -> }) else null,
    )
    HorizontalDivider(
        color = MaterialTheme.colorScheme.outline,
        thickness = 0.6.dp,
    )
}

private val privateBook = book(
    id = "private",
    title = "Pride and Prejudice",
    shared = false,
)

/** 2024-10-03T15:00:00Z, read at 2026-10-18T00:00:00Z → "Oct 3, 2024". */
private const val COMMENT_AT = 1_727_967_600_000L
private const val SHOT_NOW = 1_792_281_600_000L

private val sharedBook = book(
    id = "shared",
    title = "Emma",
    shared = true,
)

private val longTitleBook = book(
    id = "long",
    title = "The Particular Sadness of Lemon Cake and Other Stories from a Very Long Shelf",
    shared = false,
)

private val sharedWithMe = book(
    id = "theirs",
    title = "Persuasion",
    author = "Austen",
    handle = "austen",
    mine = false,
    shared = true,
    ownerUsername = "ada",
)

/** The query typed in the search goldens. Account rows are whatever this would return. */
private const val SearchQuery = "ada"

private val accountDirectory = listOf(
    AccountHit("ada", mine = true),
    AccountHit("byron"),
)

/** Username contains [query], folded the way the server folds `q`. */
private fun accountsMatching(query: String, accounts: List<AccountHit>): List<AccountHit> {
    val q = query.trim().lowercase()
    if (q.isEmpty()) return emptyList()
    return accounts.filter { it.username.lowercase().contains(q) }
}

class SearchAccountFixtureTest {
    @Test
    fun aQueryKeepsOnlyUsernamesThatContainIt() {
        assertEquals(listOf("ada"), accountsMatching("ada", accountDirectory).map { it.username })
        assertEquals(listOf("byron"), accountsMatching("BYR", accountDirectory).map { it.username })
        assertEquals(emptyList<String>(), accountsMatching("zzz", accountDirectory).map { it.username })
        assertEquals(emptyList<String>(), accountsMatching("  ", accountDirectory).map { it.username })
    }
}

private fun book(
    id: String,
    title: String,
    author: String = "Jane Austen",
    handle: String = "austen",
    mine: Boolean = true,
    shared: Boolean = false,
    ownerUsername: String? = null,
) = BookRecord(
    id = id,
    title = title,
    author = author,
    handle = handle,
    importedAt = 1,
    postCount = 120,
    progressIndex = 40,
    mine = mine,
    shared = shared,
    ownerUsername = ownerUsername,
)
