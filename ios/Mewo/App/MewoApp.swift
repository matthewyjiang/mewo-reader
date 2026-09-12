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
        .onChange(of: app.backend.kind) { _, _ in
            Task { try? await app.library.load() }
        }
        .onChange(of: app.hostedSession.session.signedIn) { old, now in
            app.hostedLibrary.wipeIfSignedOut(wasSignedIn: old, nowSignedIn: now)
            Task { try? await app.library.load() }
        }
    }
}
