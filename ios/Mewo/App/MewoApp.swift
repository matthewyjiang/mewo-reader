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
        let shots = offlineShots
        let palette = shots ? DisplayTheme.xLight.palette : app.themeStore.theme.palette
        Group {
            if shots {
                offlinePreview
            } else if app.localProfile.profile.ready {
                MewoNav()
            } else {
                OnboardingScreen()
            }
        }
        .environment(\.app, app)
        .environment(\.mewo, palette)
        .environment(\.readerScale, app.readerFont.scale)
        .preferredColorScheme(palette.isDark ? .dark : .light)
        .tint(palette.accent)
        .background(palette.ground.ignoresSafeArea())
        .onChange(of: app.backend.kind) { _, kind in
            Task { try? await app.library.load() }
            if kind == .hosted {
                Task { await app.homeFeed.load() }
            }
        }
        .onChange(of: scenePhase) { _, phase in
            app.hostedLibrary.setForeground(phase == .active)
            if phase == .active {
                Task { await app.catalog.refreshAccess() }
            }
        }
    }

    /// Screenshot launches only. Release builds do not contain the preview.
    private var offlineShots: Bool {
        #if DEBUG
        ProcessInfo.processInfo.arguments.contains("--offline-shots")
        #else
        false
        #endif
    }

    @ViewBuilder
    private var offlinePreview: some View {
        #if DEBUG
        OfflinePreview()
        #endif
    }
}
