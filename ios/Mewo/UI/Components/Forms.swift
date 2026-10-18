import SwiftUI

struct ProfilePreview: View {
    @Environment(\.mewo) private var mewo
    var name: String
    var handle: String

    var body: some View {
        let tag = LocalIdentity.normalizeHandle(handle)
        let display = name.trimmingCharacters(in: .whitespacesAndNewlines).ifBlank { tag }
        HStack(spacing: 12) {
            Avatar(name: tag.ifBlank { display }, size: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(display)
                    .font(MewoFont.bold(15))
                    .foregroundStyle(mewo.ink)
                    .lineLimit(1)
                if LocalIdentity.showsHandle(name: display, handle: tag) {
                    Text("@\(tag)")
                        .font(MewoFont.regular(13))
                        .foregroundStyle(mewo.mute)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct ProfileForm: View {
    @Environment(\.mewo) private var mewo
    @Binding var name: String
    @Binding var handle: String

    var body: some View {
        let nameErr = LocalIdentity.nameError(name)
        let handleErr = LocalIdentity.handleError(handle)
        VStack(alignment: .leading, spacing: 8) {
            field("Name", text: $name, error: nameErr, prefix: nil)
            field("Handle", text: Binding(
                get: { handle },
                set: { handle = $0.hasPrefix("@") ? String($0.dropFirst()) : $0 }
            ), error: handleErr, prefix: "@")
        }
    }

    private func field(_ label: String, text: Binding<String>, error: String?, prefix: String?) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(MewoFont.regular(13))
                .foregroundStyle(error == nil ? mewo.mute : mewo.like)
            HStack(spacing: 0) {
                if let prefix {
                    Text(prefix).font(MewoFont.regular(15)).foregroundStyle(mewo.mute)
                }
                TextField("", text: text)
                    .font(MewoFont.regular(15))
                    .foregroundStyle(mewo.ink)
                    .textInputAutocapitalization(prefix == nil ? .words : .never)
                    .autocorrectionDisabled(prefix != nil)
                    .accessibilityIdentifier(prefix == nil ? "name-field" : "handle-field")
            }
            .padding(.vertical, 10)
            overlayLine(error != nil)
            if let error {
                Text(error).font(MewoFont.regular(13)).foregroundStyle(mewo.like)
            }
        }
    }

    private func overlayLine(_ error: Bool) -> some View {
        (error ? mewo.like : mewo.mute).frame(height: 1)
    }
}

extension String {
    func ifBlank(_ fallback: () -> String) -> String {
        trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? fallback() : self
    }
}

private enum HostedAuthMode {
    case signIn, create
}

struct LibrarySettings: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @State private var mode = HostedAuthMode.signIn
    @State private var username = ""
    @State private var password = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            backendRow("On this phone", selected: app.backend.kind == .local) {
                app.backend.setKind(.local)
            }
            backendRow("Hosted server", selected: app.backend.kind == .hosted) {
                app.backend.setKind(.hosted)
            }
            if app.backend.kind == .hosted {
                hostedForm
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func backendRow(_ title: String, selected: Bool, onSelect: @escaping () -> Void) -> some View {
        Button(action: onSelect) {
            Text(title)
                .font(MewoFont.bold(17))
                .foregroundStyle(selected ? mewo.ink : mewo.mute)
                .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(title == "On this phone" ? "backend-local" : "backend-hosted")
        .accessibilityLabel(title)
    }

    private var hostedForm: some View {
        let userError = usernameFieldError(username)
        let passError = passwordFieldError(password)
        let ready = !busy && hostedAuthReady(
            username: username,
            password: password,
            baseUrl: app.hostedSession.session.baseUrl
        )
        return VStack(alignment: .leading, spacing: 0) {
            if app.hostedSession.session.signedIn {
                Text("@\(app.hostedSession.session.username ?? "")")
                    .font(MewoFont.bold(17))
                    .foregroundStyle(mewo.ink)
                    .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
                Button { run { await app.hostedAuth.signOut() } } label: {
                    Text("Sign out")
                        .font(MewoFont.bold(17))
                        .foregroundStyle(mewo.accent)
                        .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(busy)
                .accessibilityIdentifier("hosted-sign-out")
            } else {
                Text(mode == .signIn ? "Sign in" : "Create account")
                    .font(MewoFont.bold(22))
                    .foregroundStyle(mewo.ink)
                    .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
                VStack(alignment: .leading, spacing: 8) {
                    outlined("Server URL", text: Binding(
                        get: { app.hostedSession.session.baseUrl },
                        set: {
                            app.hostedSession.setBaseUrl($0)
                            error = nil
                        }
                    ), enabled: !busy, secret: false, id: "server-url", error: nil)
                    outlined("Username", text: Binding(
                        get: { username },
                        set: {
                            username = $0
                            error = nil
                        }
                    ), enabled: !busy, secret: false, id: "hosted-username", error: userError)
                    outlined("Password", text: Binding(
                        get: { password },
                        set: {
                            password = $0
                            error = nil
                        }
                    ), enabled: !busy, secret: true, id: "hosted-password", error: passError)
                }
                if let error {
                    Text(error)
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.like)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.top, 8)
                }
                PillButton(
                    title: busy ? "Connecting…" : (mode == .signIn ? "Sign in" : "Create account"),
                    enabled: ready,
                    fill: true
                ) {
                    run {
                        if mode == .signIn {
                            try await app.hostedAuth.signIn(username: username, password: password)
                        } else {
                            try await app.hostedAuth.createAccount(username: username, password: password)
                        }
                    }
                }
                .padding(.top, 12)
                Button {
                    mode = mode == .signIn ? .create : .signIn
                    error = nil
                } label: {
                    Text(mode == .signIn ? "Create an account" : "Sign in instead")
                        .font(MewoFont.regular(17))
                        .foregroundStyle(mewo.accent)
                        .frame(maxWidth: .infinity, minHeight: 40)
                }
                .buttonStyle(.plain)
                .disabled(busy)
            }
        }
        .onAppear { username = app.hostedSession.session.username ?? "" }
    }

    private func outlined(_ label: String, text: Binding<String>, enabled: Bool, secret: Bool, id: String, error: String?) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label).font(MewoFont.regular(13)).foregroundStyle(error == nil ? mewo.mute : mewo.like)
            Group {
                if secret {
                    SecureField("", text: text).disabled(!enabled)
                } else {
                    TextField("", text: text)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .disabled(!enabled)
                }
            }
            .font(MewoFont.regular(15))
            .foregroundStyle(mewo.ink)
            .padding(.vertical, 8)
            .accessibilityIdentifier(id)
            (error == nil ? mewo.mute : mewo.like).frame(height: 1)
            if let error {
                Text(error).font(MewoFont.regular(13)).foregroundStyle(mewo.like)
            }
        }
    }

    private func run(_ block: @escaping () async throws -> Void) {
        Task {
            busy = true
            error = nil
            do {
                try await block()
                password = ""
            } catch {
                self.error = error.localizedDescription
            }
            busy = false
        }
    }
}

struct DisplaySettings: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Environment(\.readerScale) private var scale
    @State private var iconError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(spacing: 12) {
                HStack(spacing: 12) {
                    swatch(.twitterLight)
                    swatch(.xLight)
                }
                HStack(spacing: 12) {
                    swatch(.twitterDark)
                    swatch(.xDark)
                }
            }
            .padding(.horizontal, 16)
            Spacer().frame(height: 28)
            fontSize
            Spacer().frame(height: 28)
            mewoMode
                .padding(.horizontal, 16)
        }
    }

    private func swatch(_ theme: DisplayTheme) -> some View {
        let palette = theme.palette
        let selected = app.themeStore.theme == theme
        return Button {
            app.themeStore.setTheme(theme)
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                ZStack {
                    Circle().fill(selected ? palette.accent : palette.line).frame(width: 24, height: 24)
                    if selected {
                        Image(systemName: "checkmark")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundStyle(palette.onAccent)
                    }
                }
                Spacer().frame(height: 20)
                HStack(spacing: 6) {
                    Circle().fill(palette.accent).frame(width: 10, height: 10)
                    Circle().fill(palette.like).frame(width: 10, height: 10)
                }
                Spacer().frame(height: 10)
                Text(theme.label)
                    .font(MewoFont.bold(14))
                    .foregroundStyle(palette.ink)
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(palette.ground, in: RoundedRectangle(cornerRadius: 12))
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(selected ? palette.accent : palette.line, lineWidth: 2)
            )
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("theme-\(theme.id)")
        .accessibilityLabel(theme.label)
    }

    private var fontSize: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Text("Font size").font(MewoFont.bold(17)).foregroundStyle(mewo.ink)
                Text(scale.talkBackLabel).font(MewoFont.regular(15)).foregroundStyle(mewo.mute)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 16)
            .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
            preview
            HStack {
                stepButton("Aa", size: 13, enabled: scale.step > ReaderTypeScale.minStep, label: "Smaller text") {
                    app.readerFont.setStep(scale.step - 1)
                }
                Slider(
                    value: Binding(
                        get: { Double(scale.step) },
                        set: { app.readerFont.setStep(Int($0.rounded())) }
                    ),
                    in: Double(ReaderTypeScale.minStep)...Double(ReaderTypeScale.maxStep),
                    step: 1
                )
                .tint(mewo.accent)
                .accessibilityLabel("Font size")
                .accessibilityValue(scale.talkBackLabel)
                stepButton("Aa", size: 22, enabled: scale.step < ReaderTypeScale.maxStep, label: "Larger text") {
                    app.readerFont.setStep(scale.step + 1)
                }
            }
            .padding(.horizontal, 8)
        }
    }

    private var preview: some View {
        VStack(spacing: 0) {
            Hairline()
            HStack(alignment: .top, spacing: 12) {
                Avatar(name: "Mewo", size: 40)
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Text("Mewo").font(MewoFont.bold(15)).lineLimit(1)
                        Text("@mewo")
                            .font(MewoFont.regular(13))
                            .foregroundStyle(mewo.mute)
                            .lineLimit(1)
                    }
                    Text("The paragraph sits at this size.")
                        .font(MewoFont.regular(scale.bodySize))
                        .lineSpacing(max(0, scale.bodyLine - scale.bodySize))
                }
            }
            .padding(.leading, 16)
            .padding(.trailing, 12)
            .padding(.vertical, 12)
            Hairline()
        }
    }

    private func stepButton(_ label: String, size: CGFloat, enabled: Bool, label a11y: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label)
                .font(.custom(MewoFont.regular, size: size))
                .foregroundStyle(enabled ? mewo.ink : mewo.mute)
                .frame(width: 48, height: 48)
        }
        .disabled(!enabled)
        .buttonStyle(.plain)
        .accessibilityLabel(a11y)
    }

    private var mewoMode: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle(isOn: Binding(
                get: { app.mewoMode.enabled },
                set: { next in
                    do {
                        try app.mewoMode.setEnabled(next)
                        iconError = nil
                    } catch {
                        iconError = error.localizedDescription.ifBlank { "Couldn't switch the app icon." }
                    }
                }
            )) {
                Text("Mewo mode").font(MewoFont.bold(17)).foregroundStyle(mewo.ink)
            }
            .tint(mewo.accent)
            .frame(minHeight: 52)
            .accessibilityIdentifier("mewo-mode")
            .accessibilityLabel("Mewo mode")
            if let iconError {
                Text(iconError).font(MewoFont.regular(15)).foregroundStyle(mewo.like)
            }
        }
    }
}
