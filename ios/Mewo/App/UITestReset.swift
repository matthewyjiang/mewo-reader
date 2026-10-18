import Foundation
import UIKit

/// Clears local prefs, books, and the hosted token when UI tests pass `--uitesting`.
enum UITestReset {
    static func runIfNeeded() {
        guard ProcessInfo.processInfo.arguments.contains("--uitesting") else { return }
        UIView.setAnimationsEnabled(false)
        for suite in ["mewo", "mewo_profile", "mewo_hosted"] {
            if let defaults = UserDefaults(suiteName: suite) {
                defaults.removePersistentDomain(forName: suite)
                defaults.synchronize()
            }
        }
        Keychain.deleteToken()
        let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        for name in ["books", "hosted", "hosted-shelves"] {
            try? FileManager.default.removeItem(at: docs.appendingPathComponent(name, isDirectory: true))
        }
        seedHostedShelf(docs: docs)
    }

    /// Screenshot and UI-test launches pass `--hosted-url=` after `--uitesting`.
    /// The shelf file is written before the store reads it, so the first frame is the cache.
    private static func seedHostedShelf(docs: URL) {
        guard let url = argument("hosted-url"), !url.isEmpty else { return }
        let user = argument("hosted-user") ?? "ada"
        let token = argument("hosted-token") ?? "token"
        if let hosted = UserDefaults(suiteName: "mewo_hosted") {
            hosted.set(url, forKey: "base_url")
            hosted.set(user, forKey: "username")
            // App init runs before the simulator keychain will take a write, so the
            // session also reads this copy. Sign-out removes it.
            hosted.set(token, forKey: "token")
        }
        Keychain.saveToken(token)
        if let profile = UserDefaults(suiteName: "mewo_profile") {
            profile.set(argument("profile-name") ?? user, forKey: "name")
            profile.set(argument("profile-handle") ?? user, forKey: "handle")
        }
        if let prefs = UserDefaults(suiteName: "mewo") {
            prefs.set("hosted", forKey: "backend_kind")
            if let theme = argument("theme") {
                prefs.set(theme, forKey: "display_theme")
            }
        }
        guard ProcessInfo.processInfo.arguments.contains("--seed-shelf") else { return }
        let book = BookRecord(
            id: "44444444-4444-4444-4444-444444444444",
            title: "A Timeline of My Own",
            author: "Mewo",
            handle: "mewo",
            importedAt: 1,
            postCount: 12,
            mine: true,
            shared: false,
            ownerUsername: user
        )
        let cache = ShelfCache(directory: docs.appendingPathComponent("hosted-shelves", isDirectory: true))
        try? cache.write(serverUrl: url, username: user, books: [book])
    }

    private static func argument(_ name: String) -> String? {
        let prefix = "--\(name)="
        guard let raw = ProcessInfo.processInfo.arguments.first(where: { $0.hasPrefix(prefix) }) else { return nil }
        return String(raw.dropFirst(prefix.count))
    }
}
