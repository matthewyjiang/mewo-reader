import Foundation
import Observation
import UIKit

enum BackendKind: String, Sendable {
    case local
    case hosted

    static func fromId(_ id: String?) -> BackendKind {
        BackendKind(rawValue: id ?? "") ?? .local
    }
}

@Observable
final class ThemeStore {
    private let defaults = UserDefaults(suiteName: "mewo") ?? .standard
    private let key = "display_theme"
    private(set) var theme: DisplayTheme

    init() {
        if let stored = defaults.string(forKey: key) {
            theme = DisplayTheme.fromId(stored)
        } else {
            let dark = UITraitCollection.current.userInterfaceStyle == .dark
            theme = dark ? .xDark : .xLight
        }
    }

    func setTheme(_ theme: DisplayTheme) {
        defaults.set(theme.id, forKey: key)
        self.theme = theme
    }
}

@Observable
final class ReaderFontStore {
    private let defaults = UserDefaults(suiteName: "mewo") ?? .standard
    private let key = "reader_font_step"
    private(set) var scale: ReaderTypeScale

    init() {
        if defaults.object(forKey: key) == nil {
            scale = .default
        } else {
            scale = ReaderTypeScale.fromStored(defaults.integer(forKey: key))
        }
    }

    func setStep(_ step: Int) {
        precondition(
            (ReaderTypeScale.minStep...ReaderTypeScale.maxStep).contains(step),
            "Font size step \(step) is outside \(ReaderTypeScale.minStep)..\(ReaderTypeScale.maxStep)"
        )
        defaults.set(step, forKey: key)
        scale = ReaderTypeScale(step: step)
    }
}

@Observable
final class BackendStore {
    private let defaults = UserDefaults(suiteName: "mewo") ?? .standard
    private let key = "backend_kind"
    private(set) var kind: BackendKind

    init() {
        kind = BackendKind.fromId(defaults.string(forKey: key))
    }

    func setKind(_ kind: BackendKind) {
        defaults.set(kind.rawValue, forKey: key)
        self.kind = kind
    }
}

@Observable
final class LocalProfileStore {
    private let defaults = UserDefaults(suiteName: "mewo_profile") ?? .standard
    private(set) var profile: LocalProfile

    init() {
        let handle = defaults.string(forKey: "handle") ?? ""
        if handle.isEmpty {
            profile = LocalProfile()
        } else {
            let name = defaults.string(forKey: "name") ?? ""
            profile = LocalIdentity.parse(name: name, handle: handle).profile ?? LocalProfile()
        }
    }

    func save(name: String, handle: String) throws {
        guard let parsed = LocalIdentity.parse(name: name, handle: handle).profile else {
            throw NSError(
                domain: "mewo",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: LocalIdentity.parse(name: name, handle: handle).error ?? "Could not save that."]
            )
        }
        defaults.set(parsed.name, forKey: "name")
        defaults.set(parsed.handle, forKey: "handle")
        profile = parsed
    }
}

@Observable
final class MewoModeStore {
    private let defaults = UserDefaults(suiteName: "mewo") ?? .standard
    private let key = "mewo_mode"
    private(set) var enabled: Bool

    init() {
        enabled = defaults.bool(forKey: key)
        applyIcon(enabled)
    }

    func setEnabled(_ enabled: Bool) throws {
        applyIcon(enabled)
        defaults.set(enabled, forKey: key)
        self.enabled = enabled
    }

    private func applyIcon(_ enabled: Bool) {
        if ProcessInfo.processInfo.arguments.contains("--uitesting") { return }
        if ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil { return }
        let name: String? = enabled ? "Teddy" : nil
        guard UIApplication.shared.supportsAlternateIcons else { return }
        if UIApplication.shared.alternateIconName == name { return }
        UIApplication.shared.setAlternateIconName(name) { error in
            if let error {
                print("Could not switch the app icon: \(error.localizedDescription)")
            }
        }
    }
}
