import SwiftUI

struct AccountDrawer<Content: View>: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Binding var open: Bool
    var gesturesEnabled: Bool
    var onOpenSettings: () -> Void
    var onOpenProfile: (String, String) -> Void
    @ViewBuilder var content: Content

    var body: some View {
        ZStack(alignment: .leading) {
            content
                .allowsHitTesting(!open)
            if !open && gesturesEnabled {
                Color.clear
                    .frame(width: 24)
                    .frame(maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(openGesture)
                    .accessibilityHidden(true)
            }
            if open {
                // Dimmer sits only to the right of the panel so a Settings/Profile
                // tap cannot land on the dismiss gesture instead.
                HStack(spacing: 0) {
                    panel
                    mewo.ink.opacity(0.2)
                        .ignoresSafeArea()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .contentShape(Rectangle())
                        .onTapGesture { withAnimation(.easeInOut(duration: 0.22)) { open = false } }
                        .gesture(closeGesture)
                        .accessibilityHidden(true)
                }
                .transition(.move(edge: .leading))
            }
        }
    }

    private var panel: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                let me = app.readerIdentity
                onOpenProfile(me.handle, me.name)
            } label: {
                VStack(alignment: .leading, spacing: 12) {
                    Avatar(name: app.readerIdentity.name, size: 56)
                    Text(app.readerIdentity.name).font(MewoFont.bold(17)).foregroundStyle(mewo.ink)
                    Text("@\(app.readerIdentity.handle)").font(MewoFont.regular(13)).foregroundStyle(mewo.mute)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.top, 8)
                .padding(.bottom, 16)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Profile")
            .accessibilityIdentifier("drawer-profile")
            Hairline()
            Button {
                onOpenSettings()
            } label: {
                HStack {
                    Text("Settings").font(MewoFont.bold(17)).foregroundStyle(mewo.ink)
                    Spacer()
                    Image(systemName: "chevron.right").foregroundStyle(mewo.mute)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 14)
                .frame(minHeight: 52)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Settings")
            .accessibilityIdentifier("drawer-settings")
            Spacer()
        }
        .frame(width: 300)
        .frame(maxHeight: .infinity)
        .background(mewo.ground)
        .ignoresSafeArea(edges: .vertical)
    }

    private var openGesture: some Gesture {
        DragGesture(minimumDistance: 16)
            .onEnded { value in
                if value.translation.width > 40 {
                    withAnimation(.easeInOut(duration: 0.22)) { open = true }
                }
            }
    }

    private var closeGesture: some Gesture {
        DragGesture(minimumDistance: 16)
            .onEnded { value in
                if value.translation.width < -40 {
                    withAnimation(.easeInOut(duration: 0.22)) { open = false }
                }
            }
    }
}
