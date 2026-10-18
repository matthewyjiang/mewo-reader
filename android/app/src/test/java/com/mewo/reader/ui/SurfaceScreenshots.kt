package com.mewo.reader.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import com.mewo.reader.data.AccountHit
import com.mewo.reader.data.BackendKind
import com.mewo.reader.data.BookRecord
import com.mewo.reader.data.FeedPost
import com.mewo.reader.data.HostedSession
import com.mewo.reader.data.LocalProfile
import com.mewo.reader.data.PostKind
import com.mewo.reader.data.ProfileRecord
import com.mewo.reader.ui.components.AccountDrawerContent
import com.mewo.reader.ui.components.AppTab
import com.mewo.reader.ui.components.BrandMark
import com.mewo.reader.ui.components.HomeTabBar
import com.mewo.reader.ui.components.MewoAppBar
import com.mewo.reader.ui.components.ProfileButton
import com.mewo.reader.ui.components.UnderlineTabs
import com.mewo.reader.ui.feed.ActivityLine
import com.mewo.reader.ui.profile.ProfileHeader
import com.mewo.reader.ui.reader.CommentItem
import com.mewo.reader.ui.reader.TimelinePost
import com.mewo.reader.ui.reader.relativeTime
import com.mewo.reader.ui.search.AccountHitRow
import com.mewo.reader.ui.library.BookPost
import com.mewo.reader.ui.library.EmptyTimeline
import com.mewo.reader.ui.library.LibraryLoading
import com.mewo.reader.ui.library.LibraryNotice
import com.mewo.reader.ui.library.RemoveBookDialog
import com.mewo.reader.ui.library.SectionLabel
import com.mewo.reader.ui.likes.EmptyLikes
import com.mewo.reader.ui.onboarding.OnboardingScreen
import com.mewo.reader.ui.profile.EmptyPosts
import com.mewo.reader.ui.search.SearchEmpty
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
import org.junit.Rule
import org.junit.Test

/**
 * Layout goldens for the surfaces an agent can check without an emulator.
 * Phone is a Pixel 5. Narrow is the same height at 320dp wide.
 */
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
        SettingsLibraryPage(onBack = {})
    }

    @Test
    fun createAccount() = shoot("create-account", signedIn = false) {
        SettingsLibraryPage(onBack = {}, startOnCreate = true)
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
        Surface {
            AccountDrawerContent(onOpenSettings = {}, onOpenProfile = { _, _ -> })
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
            AccountHitRow(AccountHit("ada", mine = true)) {}
            AccountHitRow(AccountHit("byron")) {}
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
        HomeChrome("feed") {
            ActivityLine(username = "byron") {}
            passage(reposted = true, count = 2)
            commentItem(reposted = false, repostCount = 1)
        }
    }

    @Test
    fun homeBooks() = shoot("home-books") {
        HomeChrome("books") {
            SectionLabel("My books")
            shelfBook(privateBook)
            shelfBook(sharedBook)
        }
    }

    @Test
    fun repostActive() = shoot("repost-active") {
        Surface { passage(reposted = true, count = 2) }
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
private fun SearchChrome(tab: String, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        MewoAppBar(
            leading = { ProfileButton(onClick = {}) },
            content = {
                Text(
                    text = "ada",
                    style = MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.weight(1f),
                )
            },
        )
        UnderlineTabs(
            tabs = listOf("accounts" to "Accounts", "books" to "Books", "posts" to "Posts"),
            selected = tab,
            onSelect = {},
        )
        content()
    }
}

@Composable
private fun ProfileChrome(tab: String, content: @Composable () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .verticalScroll(rememberScrollState()),
    ) {
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
        Column(Modifier.verticalScroll(rememberScrollState())) {
            content()
        }
    }
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
