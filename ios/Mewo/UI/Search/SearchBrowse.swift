import SwiftUI

enum SearchEmptyKind: Equatable {
    case none
    case noMatches
    case nothingYet
}

let browseRevalidateSeconds: TimeInterval = 60

/// Single-title headings, same strings as web `sectionFor`. Catalog keeps `catalogHeading`.
func searchHeading(_ tab: SearchTab) -> String? {
    switch tab {
    case .accounts: return "Suggested accounts"
    case .books: return "Popular on this server"
    case .posts: return "Top posts"
    case .catalog: return nil
    }
}

/// Empty copy. Same words as web `SearchEmpty.svelte`.
func searchEmptyCopy(_ tab: SearchTab, _ kind: SearchEmptyKind, query: String) -> String {
    let quoted = "“\(query.trimmingCharacters(in: .whitespacesAndNewlines))”"
    switch (tab, kind) {
    case (.accounts, .nothingYet):
        return "It's just you here so far."
    case (.accounts, .noMatches):
        return "No accounts match \(quoted)"
    case (.books, .nothingYet):
        return "No books yet."
    case (.books, .noMatches):
        return "No books match \(quoted)"
    case (.posts, .nothingYet):
        return "No posts yet. Replies and reposts on books show up here."
    case (.posts, .noMatches):
        return "No posts match \(quoted)"
    case (.catalog, _), (_, .none):
        return ""
    }
}

/// A dropped connection reads like the Catalog's. Any other error keeps its own message.
func searchErrorText(_ error: Error) -> String {
    if error is URLError {
        return catalogErrorText(code: nil, serverMessage: nil, network: true)
    }
    return error.localizedDescription
}

/// One title, 44pt, hairline. Accounts, Books, and Posts share it.
struct SectionHeading: View {
    @Environment(\.mewo) private var mewo
    var title: String

    var body: some View {
        VStack(spacing: 0) {
            Text(title)
                .font(MewoFont.bold(15))
                .foregroundStyle(mewo.ink)
                .frame(maxWidth: .infinity, alignment: .leading)
                .frame(minHeight: 44)
                .padding(.horizontal, 16)
                .accessibilityAddTraits(.isHeader)
            Hairline()
        }
        .accessibilityIdentifier("search-heading")
    }
}

/// Static stand-in rows. Same shapes as the web skeleton, without the shimmer.
struct SearchSkeleton: View {
    @Environment(\.mewo) private var mewo
    var tab: SearchTab
    var rows = 6

    var body: some View {
        VStack(spacing: 0) {
            ForEach(0..<rows, id: \.self) { _ in
                row
                Hairline()
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading results…")
        .accessibilityIdentifier("search-loading")
    }

    @ViewBuilder
    private var row: some View {
        switch tab {
        case .accounts:
            linesRow(top: 12, bottom: 12, minHeight: 64, lines: [
                BoneLine(width: 0.72, height: 14),
                BoneLine(width: 0.46, height: 12),
            ], lineHeight: 32)
        case .books:
            linesRow(top: 12, bottom: 16, minHeight: 72, lines: [
                BoneLine(width: 0.72, height: 14),
                BoneLine(width: 0.46, height: 12),
                BoneLine(width: 0.28, height: 12),
            ], lineHeight: 50)
        case .posts:
            linesRow(top: 12, bottom: 12, minHeight: 72, lines: [
                BoneLine(width: 0.72, height: 14),
                BoneLine(width: 0.88, height: 12),
                BoneLine(width: 0.46, height: 12),
            ], lineHeight: 50, alignTop: true)
        case .catalog:
            catalogRow
        }
    }

    private func linesRow(
        top: CGFloat,
        bottom: CGFloat,
        minHeight: CGFloat,
        lines: [BoneLine],
        lineHeight: CGFloat,
        alignTop: Bool = false
    ) -> some View {
        HStack(alignment: alignTop ? .top : .center, spacing: 12) {
            Circle()
                .fill(bone)
                .frame(width: 40, height: 40)
            lineStack(lines, height: lineHeight)
        }
        .padding(.horizontal, 16)
        .padding(.top, top)
        .padding(.bottom, bottom)
        .frame(minHeight: minHeight, alignment: .center)
    }

    private var catalogRow: some View {
        HStack(alignment: .center, spacing: 12) {
            RoundedRectangle(cornerRadius: 3)
                .fill(bone)
                .frame(width: 40, height: 60)
            lineStack([
                BoneLine(width: 0.60, height: 14),
                BoneLine(width: 0.40, height: 14),
            ], height: 36, spacing: 8)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private func lineStack(_ lines: [BoneLine], height: CGFloat, spacing: CGFloat = 6) -> some View {
        GeometryReader { geo in
            VStack(alignment: .leading, spacing: spacing) {
                ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                    RoundedRectangle(cornerRadius: 4)
                        .fill(bone)
                        .frame(width: geo.size.width * line.width, height: line.height)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        }
        .frame(height: height)
    }

    private var bone: Color { mewo.mute.opacity(0.16) }
}

private struct BoneLine {
    var width: CGFloat
    var height: CGFloat
}
