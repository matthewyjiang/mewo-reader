import SwiftUI

@main
struct MewoApp: App {
    @State private var env: AppEnvironment

    init() {
        UITestReset.runIfNeeded()
        _env = State(initialValue: AppEnvironment())
    }

    var body: some Scene {
        WindowGroup {
            RootView(app: env)
                .environment(\.app, env)
        }
    }
}

struct RootView: View {
    @Bindable var app: AppEnvironment
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Group {
            if app.localProfile.profile.ready {
                MewoNav()
            } else {
                OnboardingScreen()
            }
        }
        .environment(\.app, app)
        .environment(\.mewo, app.themeStore.theme.palette)
        .environment(\.readerScale, app.readerFont.scale)
        .preferredColorScheme(app.themeStore.theme.palette.isDark ? .dark : .light)
        .tint(app.themeStore.theme.palette.accent)
        .background(app.themeStore.theme.palette.ground.ignoresSafeArea())
        .onChange(of: app.backend.kind) { _, kind in
            Task { try? await app.library.load() }
            if kind == .hosted {
                Task { await app.homeFeed.load() }
            }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active {
                Task { await app.catalog.refreshAccess() }
            }
        }
    }
}
