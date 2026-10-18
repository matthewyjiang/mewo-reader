import SwiftUI

struct UnderlineTabItem: Identifiable, Equatable {
    var id: String
    var title: String
}

struct UnderlineTabs: View {
    @Environment(\.mewo) private var mewo
    var items: [UnderlineTabItem]
    var selected: String
    var idPrefix: String
    var onSelect: (String) -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                ForEach(items) { item in
                    let on = item.id == selected
                    Button {
                        onSelect(item.id)
                    } label: {
                        Text(item.title)
                            .font(MewoFont.bold(15))
                            .foregroundStyle(on ? mewo.ink : mewo.mute)
                            .frame(maxWidth: .infinity)
                            .frame(height: 48)
                            .overlay(alignment: .bottom) {
                                if on {
                                    Capsule()
                                        .fill(mewo.accent)
                                        .frame(width: 56, height: 3)
                                }
                            }
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(item.title)
                    .accessibilityIdentifier("\(idPrefix)-\(item.id)")
                }
            }
            Hairline()
        }
    }
}
