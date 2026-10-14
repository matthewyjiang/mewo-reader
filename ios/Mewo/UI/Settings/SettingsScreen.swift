import SwiftUI

struct SettingsHome: View {
    @Environment(\.mewo) private var mewo
    var onBack: () -> Void
    var onOpenAccount: () -> Void
    var onOpenDisplay: () -> Void
    var onOpenLibrary: () -> Void
    var onOpenAbout: () -> Void

    var body: some View {
        SettingsScaffold(title: "Settings", onBack: onBack) {
            VStack(spacing: 0) {
                row("Account", action: onOpenAccount)
                Hairline()
                row("Display", action: onOpenDisplay)
                Hairline()
                row("Library", action: onOpenLibrary)
                Hairline()
                row("About", action: onOpenAbout)
                Hairline()
            }
        }
    }

    private func row(_ title: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack {
                Text(title).font(MewoFont.bold(17)).foregroundStyle(mewo.ink)
                Spacer()
                Image(systemName: "chevron.right").foregroundStyle(mewo.mute)
            }
            .padding(.horizontal, 16)
            .frame(height: 52)
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
        let showPreview = !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            || !LocalIdentity.normalizeHandle(handle).isEmpty
        SettingsScaffold(title: "Account", onBack: onBack) {
            VStack(alignment: .leading, spacing: 0) {
                if showPreview {
                    ProfilePreview(name: name, handle: handle)
                    Spacer().frame(height: 12)
                }
                ProfileForm(name: $name, handle: $handle)
                Spacer().frame(height: 16)
                PillButton(title: "Save", enabled: parsed.isSuccess && dirty, fill: true) {
                    if let profile = parsed.profile {
                        do { try app.localProfile.save(name: profile.name, handle: profile.handle) }
                        catch { self.error = error.localizedDescription }
                    } else {
                        error = parsed.error ?? "Could not save that."
                    }
                }
                if let error {
                    Text(error).font(MewoFont.regular(15)).foregroundStyle(mewo.like)
                        .padding(.top, 8)
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
            DisplaySettings()
        }
    }
}

struct SettingsLibraryPage: View {
    var onBack: () -> Void
    var body: some View {
        SettingsScaffold(title: "Library", onBack: onBack) {
            LibrarySettings().padding(.horizontal, 16)
        }
    }
}

struct SettingsAboutPage: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    var onBack: () -> Void
    @State private var serverStatus = "Unavailable"

    var body: some View {
        SettingsScaffold(title: "About", onBack: onBack) {
            VStack(spacing: 0) {
                infoRow("App", body: AppBuild.displayLabel, muted: false, id: "about-app")
                Hairline()
                infoRow("Server", body: serverStatus, muted: serverStatus == "Loading…" || serverStatus == "Unavailable", id: "about-server")
                Hairline()
                infoRow("Server URL", body: serverURL, muted: serverURL == "Unavailable", id: "about-url")
                Hairline()
            }
        }
        .task(id: serverTaskID) { await loadServer() }
    }

    private var serverTaskID: String {
        "\(app.backend.kind.rawValue)|\(app.hostedSession.session.baseUrl)"
    }

    private var serverURL: String {
        let url = normalizeBaseUrl(app.hostedSession.session.baseUrl)
        return url.isEmpty ? "Unavailable" : url
    }

    private func infoRow(_ title: String, body: String, muted: Bool, id: String) -> some View {
        HStack(alignment: .center, spacing: 16) {
            Text(title)
                .font(MewoFont.bold(17))
                .foregroundStyle(mewo.ink)
                .fixedSize(horizontal: true, vertical: false)
            Text(body)
                .font(MewoFont.regular(17))
                .foregroundStyle(muted ? mewo.mute : mewo.ink)
                .multilineTextAlignment(.trailing)
                .frame(maxWidth: .infinity, alignment: .trailing)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, 16)
        .frame(maxWidth: .infinity, minHeight: 52, alignment: .center)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier(id)
        .accessibilityLabel("\(title), \(body)")
    }

    private func loadServer() async {
        guard app.backend.kind == .hosted else {
            serverStatus = "Unavailable"
            return
        }
        let base = app.hostedSession.session.baseUrl
        guard !normalizeBaseUrl(base).isEmpty else {
            serverStatus = "Unavailable"
            return
        }
        serverStatus = "Loading…"
        do {
            let info = try await app.client.serverVersion(baseUrl: base)
            if Task.isCancelled { return }
            serverStatus = info.label
        } catch is CancellationError {
            return
        } catch let error as URLError where error.code == .cancelled {
            return
        } catch {
            if Task.isCancelled { return }
            serverStatus = "Unavailable"
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
