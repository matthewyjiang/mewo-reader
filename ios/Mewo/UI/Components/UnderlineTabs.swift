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
                            .font(on ? MewoFont.bold(15) : MewoFont.regular(15))
                            .foregroundStyle(on ? mewo.ink : mewo.mute)
                            .overlay(alignment: .bottom) {
                                Capsule()
                                    .fill(on ? mewo.accent : Color.clear)
                                    .frame(height: 3)
                                    .offset(y: 8)
                            }
                            .frame(maxWidth: .infinity)
                            .frame(height: 48)
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
