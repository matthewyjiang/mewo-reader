import Foundation
import Observation
import SwiftUI

struct ReaderIdentity: Equatable {
    var name: String
    var handle: String
}

@Observable
@MainActor
final class AppEnvironment {
    let backend: BackendStore
    let hostedSession: HostedSessionStore
    let hostedAuth: HostedAuth
    let themeStore: ThemeStore
    let mewoMode: MewoModeStore
    let readerFont: ReaderFontStore
    let localProfile: LocalProfileStore
    let localLibrary: LocalLibraryStore
    let hostedLibrary: HostedLibraryStore
    let library: SwitchingLibraryStore
    let client = HostedClient()

    init() {
        backend = BackendStore()
        hostedSession = HostedSessionStore()
        hostedAuth = HostedAuth(sessions: hostedSession, client: client)
        themeStore = ThemeStore()
        mewoMode = MewoModeStore()
        readerFont = ReaderFontStore()
        localProfile = LocalProfileStore()
        let opener = EpubOpener()
        let files = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        localLibrary = LocalLibraryStore(opener: opener, profile: localProfile, files: files)
        hostedLibrary = HostedLibraryStore(opener: opener, sessions: hostedSession, client: client, files: files)
        library = SwitchingLibraryStore(local: localLibrary, hosted: hostedLibrary, backend: backend)
    }

    var readerIdentity: ReaderIdentity {
        if backend.kind == .hosted, let username = hostedSession.session.username, !username.isEmpty {
            return ReaderIdentity(name: username, handle: username)
        }
        return ReaderIdentity(name: localProfile.profile.displayName, handle: localProfile.profile.handle)
    }

    func commenterIdentity(_ comment: PostComment) -> ReaderIdentity {
        if backend.kind != .hosted {
            return readerIdentity
        }
        return ReaderIdentity(name: comment.username, handle: comment.username)
    }
}

private struct AppEnvironmentKey: EnvironmentKey {
    static let defaultValue: AppEnvironment? = nil
}

extension EnvironmentValues {
    @MainActor
    var app: AppEnvironment {
        get { self[AppEnvironmentKey.self] ?? AppEnvironment() }
        set { self[AppEnvironmentKey.self] = newValue }
    }
}
