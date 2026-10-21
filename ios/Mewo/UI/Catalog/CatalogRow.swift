import SwiftUI

struct CatalogRow: View {
    @Environment(\.mewo) private var mewo
    var work: CatalogWork
    var request: BookRequest?
    var onOpen: () -> Void
    var onAct: () -> Void

    private var action: RowAction { rowAction(request, work.onServer) }
    private var label: String? { Mewo.badge(work, request) }
    private var symbol: String? { rowSymbol(action) }

    var body: some View {
        HStack(alignment: .center, spacing: 0) {
            Button(action: onOpen) {
                HStack(alignment: .center, spacing: 12) {
                    CatalogCover(title: work.title, coverId: work.coverId)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(work.title)
                            .font(MewoFont.bold(17))
                            .foregroundStyle(mewo.ink)
                            .lineLimit(1)
                        Text(meta)
                            .font(MewoFont.regular(15))
                            .foregroundStyle(mewo.mute)
                            .lineLimit(1)
                        if let label {
                            HStack(spacing: 6) {
                                Circle()
                                    .fill(dot)
                                    .frame(width: 8, height: 8)
                                Text(label)
                                    .font(MewoFont.regular(13))
                                    .foregroundStyle(mewo.ink)
                                    .lineLimit(1)
                            }
                        }
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(catalogRowLabel(work, request))
            .accessibilityIdentifier("catalog-\(catalogOlid(work.workKey))")
            if let symbol {
                Button(action: onAct) {
                    Image(systemName: symbol)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(mewo.accent)
                        .frame(width: 32, height: 32)
                        .overlay(Circle().strokeBorder(mewo.accent, lineWidth: 1.5))
                        .frame(width: 44, height: 44)
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(rowActionLabel(action, work, request))
                .accessibilityIdentifier("catalog-act-\(catalogOlid(work.workKey))")
            }
        }
        .padding(.leading, 16)
        .padding(.trailing, symbol == nil ? 16 : 4)
        .padding(.vertical, 12)
    }

    private var meta: String {
        catalogMeta(author: work.author, year: work.year)
    }

    private var dot: Color {
        guard let label else { return mewo.mute }
        switch badgeTone(label) {
        case .shelf: return mewo.repost
        case .failed: return mewo.like
        case .server: return mewo.accent
        case .progress: return mewo.mute
        }
    }
}
