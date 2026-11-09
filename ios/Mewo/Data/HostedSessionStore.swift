import Foundation
import Observation

struct HostedSession: Equatable, Sendable {
    var baseUrl: String = ""
    var username: String?
    var token: String?
    /// Set by a 401. Sign-in and sign-out clear it. The outbox never reads it.
    var expired: OutboxAccount? = nil

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
    /// Who owns the shelf and downloads on disk. Sign-in does not clear this.
    /// The library replaces it only after that account's files are deleted.
    private(set) var filesOnDisk: OutboxAccount?
    /// Fires after the URL, account, or sign-in changes. The library drops the previous shelf before the next refresh.
    var onSessionChange: ((HostedSession, HostedSession) -> Void)?
    private var listeners: [UUID: (HostedSession, HostedSession) -> Void] = [:]

    /// Extra observers. The library keeps the single `onSessionChange` slot.
    @discardableResult
    func addListener(_ body: @escaping (HostedSession, HostedSession) -> Void) -> UUID {
        let id = UUID()
        listeners[id] = body
        return id
    }

    func removeListener(_ id: UUID) {
        listeners[id] = nil
    }

    init(
        defaults: UserDefaults = UserDefaults(suiteName: "mewo_hosted") ?? .standard,
        persistToken: Bool = true,
        uitesting: Bool = ProcessInfo.processInfo.arguments.contains("--uitesting")
    ) {
        self.defaults = defaults
        self.persistToken = persistToken
        // The keychain is the real token. UI tests also leave a copy in defaults
        // because a write during app launch does not stick in an unsigned simulator.
        let seeded = uitesting ? defaults.string(forKey: "token").flatMap { $0.isEmpty ? nil : $0 } : nil
        let expiredUrl = defaults.string(forKey: "expired_url") ?? ""
        let expiredUser = defaults.string(forKey: "expired_user") ?? ""
        session = HostedSession(
            baseUrl: defaults.string(forKey: "base_url") ?? "",
            username: defaults.string(forKey: "username"),
            token: (persistToken ? Keychain.readToken() : nil) ?? seeded,
            expired: expiredUrl.isEmpty || expiredUser.isEmpty
                ? nil
                : OutboxAccount(url: expiredUrl, username: expiredUser)
        )
        let filesUrl = defaults.string(forKey: "files_url") ?? ""
        let filesUser = defaults.string(forKey: "files_user") ?? ""
        filesOnDisk = filesUrl.isEmpty || filesUser.isEmpty
            ? nil
            : OutboxAccount(url: filesUrl, username: filesUser)
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
        clearExpired()
        session.username = username
        session.token = token
        notify(previous)
    }

    /// A 401. Deletes the token and keeps the account so downloads can stay.
    /// With no username there is nothing to resume, so the saved account is cleared.
    func expire() {
        let previous = session
        defaults.removeObject(forKey: "username")
        defaults.removeObject(forKey: "token")
        if persistToken { Keychain.deleteToken() }
        session.username = nil
        session.token = nil
        if let account = previous.outboxAccount {
            defaults.set(account.url, forKey: "expired_url")
            defaults.set(account.username, forKey: "expired_user")
            session.expired = account
        } else {
            clearExpired()
        }
        notify(previous)
    }

    func signOut() {
        let previous = session
        defaults.removeObject(forKey: "username")
        defaults.removeObject(forKey: "token")
        if persistToken { Keychain.deleteToken() }
        clearExpired()
        session.username = nil
        session.token = nil
        notify(previous)
    }

    /// Records the account whose files are on disk. Does not notify listeners.
    func noteFilesOnDisk(_ account: OutboxAccount?) {
        if let account {
            defaults.set(account.url, forKey: "files_url")
            defaults.set(account.username, forKey: "files_user")
        } else {
            defaults.removeObject(forKey: "files_url")
            defaults.removeObject(forKey: "files_user")
        }
        filesOnDisk = account
    }

    private func clearExpired() {
        defaults.removeObject(forKey: "expired_url")
        defaults.removeObject(forKey: "expired_user")
        session.expired = nil
    }

    private func notify(_ previous: HostedSession) {
        guard previous != session else { return }
        onSessionChange?(previous, session)
        for listener in listeners.values {
            listener(previous, session)
        }
    }
}
