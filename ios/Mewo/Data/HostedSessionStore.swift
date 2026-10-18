import Foundation
import Observation

struct HostedSession: Equatable, Sendable {
    var baseUrl: String = ""
    var username: String?
    var token: String?

    var signedIn: Bool {
        !(token ?? "").isEmpty && normalizedUrl() != nil
    }

    func normalizedUrl() -> String? {
        let url = normalizeBaseUrl(baseUrl)
        return url.isEmpty ? nil : url
    }
}

func normalizeBaseUrl(_ raw: String) -> String {
    var url = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    while url.hasSuffix("/") { url.removeLast() }
    if url.isEmpty { return "" }
    if !url.hasPrefix("http://") && !url.hasPrefix("https://") {
        url = "http://\(url)"
    }
    return url
}

@Observable
final class HostedSessionStore {
    private let defaults: UserDefaults
    private let persistToken: Bool
    private(set) var session: HostedSession
    /// Fires after the URL, account, or sign-in changes. The library drops the previous shelf before the next refresh.
    var onSessionChange: ((HostedSession, HostedSession) -> Void)?

    init(
        defaults: UserDefaults = UserDefaults(suiteName: "mewo_hosted") ?? .standard,
        persistToken: Bool = true
    ) {
        self.defaults = defaults
        self.persistToken = persistToken
        session = HostedSession(
            baseUrl: defaults.string(forKey: "base_url") ?? "",
            username: defaults.string(forKey: "username"),
            token: persistToken ? Keychain.readToken() : nil
        )
    }

    func setBaseUrl(_ url: String) {
        let previous = session
        defaults.set(url, forKey: "base_url")
        session.baseUrl = url
        notify(previous)
    }

    func setSignedIn(username: String, token: String) {
        let previous = session
        defaults.set(username, forKey: "username")
        if persistToken { Keychain.saveToken(token) }
        session.username = username
        session.token = token
        notify(previous)
    }

    func signOut() {
        let previous = session
        defaults.removeObject(forKey: "username")
        if persistToken { Keychain.deleteToken() }
        session.username = nil
        session.token = nil
        notify(previous)
    }

    private func notify(_ previous: HostedSession) {
        guard previous != session else { return }
        onSessionChange?(previous, session)
    }
}
