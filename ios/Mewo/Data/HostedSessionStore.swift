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
    private let defaults = UserDefaults(suiteName: "mewo_hosted") ?? .standard
    private(set) var session: HostedSession

    init() {
        session = HostedSession(
            baseUrl: defaults.string(forKey: "base_url") ?? "",
            username: defaults.string(forKey: "username"),
            token: Keychain.readToken()
        )
    }

    func setBaseUrl(_ url: String) {
        defaults.set(url, forKey: "base_url")
        session.baseUrl = url
    }

    func setSignedIn(username: String, token: String) {
        defaults.set(username, forKey: "username")
        Keychain.saveToken(token)
        session.username = username
        session.token = token
    }

    func signOut() {
        defaults.removeObject(forKey: "username")
        Keychain.deleteToken()
        session.username = nil
        session.token = nil
    }
}
