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
        for name in ["books", "hosted"] {
            try? FileManager.default.removeItem(at: docs.appendingPathComponent(name, isDirectory: true))
        }
    }
}
