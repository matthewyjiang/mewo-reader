import SwiftUI

struct MewoPalette: Equatable, Sendable {
    var ground: Color
    var ink: Color
    var mute: Color
    var line: Color
    var hover: Color
    var accent: Color
    var like: Color
    var repost: Color
    var onAccent: Color
    var isDark: Bool
}

enum DisplayTheme: String, CaseIterable, Identifiable, Sendable {
    case twitterLight = "twitter_light"
    case twitterDark = "twitter_dim"
    case xLight = "x_light"
    case xDark = "x_lights_out"

    var id: String { rawValue }

    var label: String {
        switch self {
        case .twitterLight: return "Twitter light"
        case .twitterDark: return "Twitter dim"
        case .xLight: return "X light"
        case .xDark: return "Lights out"
        }
    }

    var palette: MewoPalette {
        switch self {
        case .twitterLight:
            MewoPalette(
                ground: Color(hex: 0xFFFFFF), ink: Color(hex: 0x14171A), mute: Color(hex: 0x657786),
                line: Color(hex: 0xE1E8ED), hover: Color(hex: 0xF5F8FA), accent: Color(hex: 0x1DA1F2),
                like: Color(hex: 0xE0245E), repost: Color(hex: 0x17BF63), onAccent: Color(hex: 0xFFFFFF),
                isDark: false
            )
        case .twitterDark:
            MewoPalette(
                ground: Color(hex: 0x15202B), ink: Color(hex: 0xF7F9F9), mute: Color(hex: 0x8B98A5),
                line: Color(hex: 0x38444D), hover: Color(hex: 0x1E2732), accent: Color(hex: 0x1DA1F2),
                like: Color(hex: 0xE0245E), repost: Color(hex: 0x17BF63), onAccent: Color(hex: 0xFFFFFF),
                isDark: true
            )
        case .xLight:
            MewoPalette(
                ground: Color(hex: 0xFFFFFF), ink: Color(hex: 0x0F1419), mute: Color(hex: 0x536471),
                line: Color(hex: 0xEFF3F4), hover: Color(hex: 0xF7F9F9), accent: Color(hex: 0x1D9BF0),
                like: Color(hex: 0xF91880), repost: Color(hex: 0x00BA7C), onAccent: Color(hex: 0xFFFFFF),
                isDark: false
            )
        case .xDark:
            MewoPalette(
                ground: Color(hex: 0x000000), ink: Color(hex: 0xE7E9EA), mute: Color(hex: 0x71767B),
                line: Color(hex: 0x2F3336), hover: Color(hex: 0x16181C), accent: Color(hex: 0x1D9BF0),
                like: Color(hex: 0xF91880), repost: Color(hex: 0x00BA7C), onAccent: Color(hex: 0xFFFFFF),
                isDark: true
            )
        }
    }

    static func fromId(_ id: String?) -> DisplayTheme {
        DisplayTheme(rawValue: id ?? "") ?? .xDark
    }
}

let avatarPalette: [Color] = [
    Color(hex: 0x1D9BF0),
    Color(hex: 0xF91880),
    Color(hex: 0x00BA7C),
    Color(hex: 0xFF7A00),
    Color(hex: 0x7856FF),
    Color(hex: 0xFFAD1F),
]

let twitterBlue = Color(hex: 0x1DA1F2)

extension Color {
    init(hex: UInt32, alpha: Double = 1) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: alpha
        )
    }
}

private struct PaletteKey: EnvironmentKey {
    static let defaultValue = DisplayTheme.xDark.palette
}

extension EnvironmentValues {
    var mewo: MewoPalette {
        get { self[PaletteKey.self] }
        set { self[PaletteKey.self] = newValue }
    }
}
