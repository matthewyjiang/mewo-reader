#if DEBUG
import SwiftUI

/// The Library form in one email state, picked by `--email-shots=<state>`. UI tests screenshot this.
struct EmailPreview: View {
    @Environment(\.mewo) private var mewo
    var state: String

    var body: some View {
        ScrollView {
            settings
                .padding(.horizontal, 16)
        }
        .background(mewo.ground)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("email-preview")
    }

    @ViewBuilder
    private var settings: some View {
        let mailOn = InstanceInfo(allowSignup: true, passwordReset: true)
        switch state {
        case "reset":
            LibrarySettings(startMode: .reset, instance: mailOn)
        case "create":
            LibrarySettings(startMode: .create, instance: mailOn)
        case "account":
            LibrarySettings(me: Me(
                username: "ada",
                isAdmin: false,
                canRequest: false,
                email: "ada@example.com",
                passwordReset: true
            ))
        default:
            LibrarySettings(instance: mailOn)
        }
    }

    static var requested: String? {
        let prefix = "--email-shots="
        return ProcessInfo.processInfo.arguments
            .first { $0.hasPrefix(prefix) }
            .map { String($0.dropFirst(prefix.count)) }
    }
}
#endif
