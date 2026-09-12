import SwiftUI
import UIKit

@Observable
final class HideOnScrollState {
    var visible = true
    private var accumulated: CGFloat = 0
    private let slop: CGFloat = 10
    private var enabled: Bool

    init() {
        enabled = !UIAccessibility.isVoiceOverRunning
    }

    func show() {
        visible = true
        accumulated = 0
    }

    func ingest(delta: CGFloat) {
        guard enabled, delta != 0 else { return }
        accumulated += delta
        if accumulated >= slop {
            visible = false
            accumulated = 0
        } else if accumulated <= -slop {
            visible = true
            accumulated = 0
        }
    }

    func showWhenIdle(_ idle: Bool) {
        if idle { show() }
    }

    func bindTop(_ offset: CGFloat) {
        if offset <= 0 { show() }
    }
}

struct Hairline: View {
    @Environment(\.mewo) private var mewo
    var body: some View {
        mewo.line.frame(height: 0.6)
    }
}

struct MewoAppBar<Leading: View, Trailing: View, Center: View, Content: View>: View {
    @Environment(\.mewo) private var mewo
    var visible: Bool = true
    var leading: Leading
    var trailing: Trailing
    var center: Center
    var content: Content?

    init(
        visible: Bool = true,
        @ViewBuilder leading: () -> Leading,
        @ViewBuilder trailing: () -> Trailing = { EmptyView() },
        @ViewBuilder center: () -> Center = { EmptyView() },
        @ViewBuilder content: () -> Content
    ) {
        self.visible = visible
        self.leading = leading()
        self.trailing = trailing()
        self.center = center()
        self.content = content()
    }

    var body: some View {
        VStack(spacing: 0) {
            if visible {
                Group {
                    if let content {
                        HStack(spacing: 0) {
                            leading
                            content
                            trailing
                        }
                        .frame(height: 56)
                    } else {
                        ZStack {
                            HStack(spacing: 0) {
                                leading
                                Spacer(minLength: 0)
                                trailing
                            }
                            center
                        }
                        .frame(height: 56)
                    }
                }
                .transition(.move(edge: .top).combined(with: .opacity))
                Hairline()
            }
        }
        .background(mewo.ground)
        .animation(.easeInOut(duration: 0.2), value: visible)
    }
}

extension MewoAppBar where Content == EmptyView {
    init(
        visible: Bool = true,
        @ViewBuilder leading: () -> Leading,
        @ViewBuilder trailing: () -> Trailing = { EmptyView() },
        @ViewBuilder center: () -> Center = { EmptyView() }
    ) {
        self.visible = visible
        self.leading = leading()
        self.trailing = trailing()
        self.center = center()
        self.content = nil
    }
}

enum AppTab: Hashable {
    case home, search, likes
}

struct HomeTabBar: View {
    @Environment(\.mewo) private var mewo
    var selected: AppTab
    var visible: Bool
    var onSelect: (AppTab) -> Void

    var body: some View {
        VStack(spacing: 0) {
            if visible {
                VStack(spacing: 0) {
                    Hairline()
                    HStack {
                        tab(.home, filled: "house.fill", idle: "house", label: "Home")
                        tab(.search, filled: "magnifyingglass", idle: "magnifyingglass", label: "Search")
                        tab(.likes, filled: "heart.fill", idle: "heart", label: "Likes")
                    }
                    .frame(height: 52)
                }
                .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .background(mewo.ground)
        .animation(.easeInOut(duration: 0.2), value: visible)
    }

    private func tab(_ tab: AppTab, filled: String, idle: String, label: String) -> some View {
        Button {
            onSelect(tab)
        } label: {
            Image(systemName: selected == tab ? filled : idle)
                .font(.system(size: 22))
                .foregroundStyle(selected == tab ? mewo.ink : mewo.mute)
                .frame(maxWidth: .infinity, minHeight: 48)
        }
        .accessibilityLabel(label)
        .accessibilityIdentifier("tab-\(label.lowercased())")
        .buttonStyle(.plain)
    }
}

struct Avatar: View {
    var name: String
    var cover: URL?
    var size: CGFloat = 40

    var body: some View {
        let color = avatarPalette[abs(name.hashValue) % avatarPalette.count]
        let letter = String(name.trimmingCharacters(in: .whitespacesAndNewlines).first ?? "M").uppercased()
        ZStack {
            color
            if let cover, let image = UIImage(contentsOfFile: cover.path) {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFill()
            } else {
                Text(letter)
                    .font(.custom(MewoFont.bold, size: size * 0.42))
                    .foregroundStyle(Color(hex: 0xFFFFFF))
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
    }
}

struct ProfileButton: View {
    @Environment(\.app) private var app
    var onClick: () -> Void

    var body: some View {
        Button(action: onClick) {
            Avatar(name: app.readerIdentity.name, size: 32)
                .padding(.horizontal, 12)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Account")
        .accessibilityIdentifier("account-button")
    }
}

struct PillButton: View {
    @Environment(\.mewo) private var mewo
    var title: String
    var enabled: Bool
    var action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(MewoFont.bold(14))
                .foregroundStyle(mewo.onAccent)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .background(mewo.accent.opacity(enabled ? 1 : 0.35), in: Capsule())
        }
        .disabled(!enabled)
        .buttonStyle(.plain)
        .accessibilityLabel(title)
        .accessibilityIdentifier(title)
    }
}
