import SwiftUI

enum Route: Hashable {
    case reader(bookId: String, postId: String?)
    case profile(handle: String, name: String)
    case settings
    case settingsAccount
    case settingsDisplay
    case settingsLibrary
}

struct MewoNav: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @State private var tab: AppTab = .home
    @State private var path: [Route] = []
    @State private var drawerOpen = false
    @State private var chrome = HideOnScrollState()

    private var onCover: Bool {
        if let last = path.last {
            switch last {
            case .reader, .settings, .settingsAccount, .settingsDisplay, .settingsLibrary:
                return true
            case .profile:
                return true
            }
        }
        return false
    }

    private var hideTabs: Bool {
        guard let last = path.last else { return false }
        switch last {
        case .settings, .settingsAccount, .settingsDisplay, .settingsLibrary, .reader:
            return true
        case .profile:
            return false
        }
    }

    var body: some View {
        AccountDrawer(
            open: $drawerOpen,
            gesturesEnabled: path.isEmpty,
            onOpenSettings: { push(.settings) },
            onOpenProfile: { handle, name in push(.profile(handle: handle, name: name)) }
        ) {
            VStack(spacing: 0) {
                NavigationStack(path: $path) {
                    tabRoot
                        .navigationDestination(for: Route.self) { route in
                            destination(route)
                        }
                        .navigationBarHidden(true)
                }
                HomeTabBar(
                    selected: tab,
                    visible: chrome.visible && !hideTabs,
                    onSelect: goTab
                )
            }
            .background(mewo.ground)
        }
        .onChange(of: path) { _, _ in
            chrome.show()
            drawerOpen = false
        }
    }

    private var isProfile: Bool {
        if case .profile = path.last { return true }
        return false
    }

    @ViewBuilder
    private var tabRoot: some View {
        switch tab {
        case .home:
            LibraryScreen(
                env: app,
                chrome: chrome,
                onOpenBook: { push(.reader(bookId: $0, postId: nil)) },
                onOpenAccount: { withAnimation(.easeInOut(duration: 0.22)) { drawerOpen = true } }
            )
            .coordinateSpace(name: "library")
        case .search:
            SearchScreen(
                env: app,
                chrome: chrome,
                onOpenBook: { push(.reader(bookId: $0, postId: nil)) },
                onOpenPost: { book, post in push(.reader(bookId: book, postId: post)) },
                onOpenAccount: { withAnimation(.easeInOut(duration: 0.22)) { drawerOpen = true } },
                onOpenProfile: { handle, name in push(.profile(handle: handle, name: name)) }
            )
        case .likes:
            LikesScreen(
                env: app,
                chrome: chrome,
                onOpenPost: { book, post in push(.reader(bookId: book, postId: post)) },
                onOpenAccount: { withAnimation(.easeInOut(duration: 0.22)) { drawerOpen = true } },
                onOpenProfile: { handle, name in push(.profile(handle: handle, name: name)) }
            )
        }
    }

    @ViewBuilder
    private func destination(_ route: Route) -> some View {
        switch route {
        case .reader(let bookId, let postId):
            ReaderScreen(
                env: app,
                bookId: bookId,
                postId: postId,
                chrome: chrome,
                onBack: { path.removeLast() },
                onOpenProfile: { handle, name in push(.profile(handle: handle, name: name)) }
            )
            .navigationBarHidden(true)
        case .profile(let handle, let name):
            ProfileScreen(
                env: app,
                handle: handle,
                displayName: name,
                chrome: chrome,
                onBack: { path.removeLast() },
                onOpenProfile: { nextHandle, nextName in
                    if nextHandle.caseInsensitiveCompare(handle) != .orderedSame {
                        push(.profile(handle: nextHandle, name: nextName))
                    }
                }
            )
            .navigationBarHidden(true)
        case .settings:
            SettingsHome(
                onBack: { path.removeLast() },
                onOpenAccount: { path.append(.settingsAccount) },
                onOpenDisplay: { path.append(.settingsDisplay) },
                onOpenLibrary: { path.append(.settingsLibrary) }
            )
            .navigationBarHidden(true)
        case .settingsAccount:
            SettingsAccountPage(onBack: { path.removeLast() }).navigationBarHidden(true)
        case .settingsDisplay:
            SettingsDisplayPage(onBack: { path.removeLast() }).navigationBarHidden(true)
        case .settingsLibrary:
            SettingsLibraryPage(onBack: { path.removeLast() }).navigationBarHidden(true)
        }
    }

    private func goTab(_ next: AppTab) {
        chrome.show()
        tab = next
        path.removeAll()
    }

    private func push(_ route: Route) {
        chrome.show()
        if case .profile(let handle, _) = route, case .profile(let current, _) = path.last,
           handle.caseInsensitiveCompare(current) == .orderedSame {
            drawerOpen = false
            return
        }
        // Closing the drawer in the same transaction as a NavigationStack push
        // used to animate the destination away. Commit both without animation.
        var transaction = Transaction()
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            path.append(route)
            drawerOpen = false
        }
    }
}
