import SwiftUI

struct CatalogRow: View {
    @Environment(\.mewo) private var mewo
    var work: CatalogWork
    var request: BookRequest?
    var onOpen: () -> Void
    var onAct: () -> Void

    private var action: RowAction { rowAction(request, work.onServer) }
    private var label: String? { Mewo.badge(work, request) }

    private var actionName: String {
        switch action {
        case .request: return "Request"
        case .pending: return request.map { stateLabel($0) } ?? "Request"
        case .open: return openLabel(work, request)
        case .retry: return "Retry"
        }
    }

    private var symbol: String {
        switch action {
        case .request: return "plus"
        case .pending: return "clock"
        case .open: return "checkmark"
        case .retry: return "arrow.clockwise"
        }
    }

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
            Button(action: onAct) {
                Image(systemName: symbol)
                    .font(.system(size: 18))
                    .foregroundStyle(mewo.mute)
                    .frame(width: 44, height: 44)
            }
            .buttonStyle(.plain)
            .disabled(action == .pending)
            .accessibilityLabel(actionName)
            .accessibilityIdentifier("catalog-act-\(catalogOlid(work.workKey))")
        }
        .padding(.leading, 16)
        .padding(.trailing, 4)
        .padding(.vertical, 8)
    }

    private var meta: String {
        if let year = work.year { return "\(work.author) · \(year)" }
        return work.author
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
