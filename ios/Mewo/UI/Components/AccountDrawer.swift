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
            // Full-bleed dimmer, faded in place. It used to share the sliding
            // stack with the 300pt panel, so its frame was only the leftover
            // width and the move transition carried that strip across the
            // library. The panel stays above it, so Profile and Settings
            // still win their taps.
            scrim
            if !open && gesturesEnabled {
                Color.clear
                    .frame(width: 24)
                    .frame(maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(openGesture)
                    .accessibilityHidden(true)
            }
            if open {
                panel
                    .transition(.move(edge: .leading))
                    .zIndex(1)
            }
        }
    }

    private var scrim: some View {
        // Layout stays in the safe area so this layer cannot stretch the
        // panel up over the clock. The fill still runs under the status bar.
        Color.clear
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle())
            .onTapGesture { withAnimation(.easeInOut(duration: 0.22)) { open = false } }
            .gesture(closeGesture)
            .background(mewo.ink.opacity(0.2), ignoresSafeAreaEdges: .all)
            .opacity(open ? 1 : 0)
            .allowsHitTesting(open)
            .accessibilityHidden(true)
    }

    private var panel: some View {
        let me = app.readerIdentity
        return VStack(alignment: .leading, spacing: 0) {
            Button {
                onOpenProfile(me.handle, me.name)
            } label: {
                VStack(alignment: .leading, spacing: 4) {
                    Avatar(name: me.name, size: 64)
                    Text(me.name)
                        .font(MewoFont.bold(20))
                        .foregroundStyle(mewo.ink)
                        .padding(.top, 8)
                    Text("@\(me.handle)")
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.mute)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.top, 8)
                .padding(.bottom, 20)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Profile")
            .accessibilityIdentifier("drawer-header")
            drawerRow(symbol: "person", title: "Profile", identifier: "drawer-profile") {
                onOpenProfile(me.handle, me.name)
            }
            drawerRow(symbol: "gearshape", title: "Settings", identifier: "drawer-settings") {
                onOpenSettings()
            }
            Spacer(minLength: 0)
        }
        .frame(width: 300)
        .frame(maxHeight: .infinity, alignment: .top)
        // Fill behind the clock and the home indicator. The avatar stays
        // in the safe area; ignoring safe area on the whole panel pulled it
        // up over the status bar.
        .background(mewo.ground, ignoresSafeAreaEdges: .vertical)
    }

    private func drawerRow(symbol: String, title: String, identifier: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(alignment: .center, spacing: 16) {
                Image(systemName: symbol)
                    .font(.system(size: 27))
                    .frame(width: 32, height: 32, alignment: .center)
                Text(title)
                    .font(MewoFont.bold(21))
            }
            .foregroundStyle(mewo.ink)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .frame(height: 68)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(title)
        .accessibilityIdentifier(identifier)
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
