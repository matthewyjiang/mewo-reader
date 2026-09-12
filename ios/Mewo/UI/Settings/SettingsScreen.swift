import SwiftUI

struct SettingsHome: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    var onBack: () -> Void
    var onOpenAccount: () -> Void
    var onOpenDisplay: () -> Void
    var onOpenLibrary: () -> Void

    var body: some View {
        SettingsScaffold(title: "Settings", onBack: onBack) {
            row("Account", body: app.localProfile.profile.ready ? "@\(app.localProfile.profile.handle)" : "Name and handle.", action: onOpenAccount)
            Hairline()
            row("Display", body: "Twitter or X. Light or night. Font size. Mewo mode.", action: onOpenDisplay)
            Hairline()
            row("Library", body: "On this phone, or a hosted server.", action: onOpenLibrary)
            Hairline()
        }
    }

    private func row(_ title: String, body: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text(title).font(MewoFont.bold(17)).foregroundStyle(mewo.ink)
                    Text(body).font(MewoFont.regular(13)).foregroundStyle(mewo.mute)
                }
                Spacer()
                Image(systemName: "chevron.right").foregroundStyle(mewo.mute)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
            .frame(minHeight: 52)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("settings-\(title.lowercased())")
        .accessibilityLabel(title)
    }
}

struct SettingsAccountPage: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    var onBack: () -> Void
    @State private var name = ""
    @State private var handle = ""
    @State private var error: String?

    var body: some View {
        let parsed = LocalIdentity.parse(name: name, handle: handle)
        let dirty = name.trimmingCharacters(in: .whitespacesAndNewlines) != app.localProfile.profile.name
            || LocalIdentity.normalizeHandle(handle) != app.localProfile.profile.handle
        SettingsScaffold(title: "Account", onBack: onBack) {
            VStack(alignment: .leading, spacing: 16) {
                ProfilePreview(name: name, handle: handle)
                ProfileForm(name: $name, handle: $handle)
                HStack {
                    Spacer()
                    PillButton(title: "Save", enabled: parsed.isSuccess && dirty) {
                        if let profile = parsed.profile {
                            do { try app.localProfile.save(name: profile.name, handle: profile.handle) }
                            catch { self.error = error.localizedDescription }
                        } else {
                            error = parsed.error ?? "Could not save that."
                        }
                    }
                }
                if let error {
                    Text(error).font(MewoFont.regular(15)).foregroundStyle(mewo.like)
                }
            }
            .padding(16)
        }
        .onAppear {
            name = app.localProfile.profile.name
            handle = app.localProfile.profile.handle
        }
    }
}

struct SettingsDisplayPage: View {
    var onBack: () -> Void
    var body: some View {
        SettingsScaffold(title: "Display", onBack: onBack) {
            DisplaySettings().padding(.vertical, 16)
        }
    }
}

struct SettingsLibraryPage: View {
    var onBack: () -> Void
    var body: some View {
        SettingsScaffold(title: "Library", onBack: onBack) {
            LibrarySettings().padding(16)
        }
    }
}

struct SettingsScaffold<Content: View>: View {
    @Environment(\.mewo) private var mewo
    var title: String
    var onBack: () -> Void
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) {
            MewoAppBar(
                leading: {
                    Button(action: onBack) {
                        Image(systemName: "chevron.left")
                            .font(.system(size: 18, weight: .semibold))
                            .foregroundStyle(mewo.ink)
                            .frame(width: 44, height: 44)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Back")
                },
                center: {
                    Text(title)
                        .font(MewoFont.bold(15))
                        .foregroundStyle(mewo.ink)
                        .accessibilityIdentifier(title == "Settings" ? "settings-home" : "settings-page")
                }
            )
            ScrollView {
                content.padding(.bottom, 28)
            }
        }
        .background(mewo.ground)
    }
}
