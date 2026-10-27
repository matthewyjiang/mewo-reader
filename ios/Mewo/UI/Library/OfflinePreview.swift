import SwiftUI

/// Shelf rows and the Settings line, one of each offline state. UI tests screenshot this.
struct OfflinePreview: View {
    @Environment(\.mewo) private var mewo

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                shelf(shotBook("Pride and Prejudice", id: "private", shared: false), .notDownloaded, mine: true)
                Hairline()
                shelf(shotBook("Emma", id: "shared", shared: true), .downloading(0.4), mine: true)
                Hairline()
                shelf(
                    shotBook(
                        "The Particular Sadness of Lemon Cake and Other Stories from a Very Long Shelf",
                        id: "long",
                        shared: false
                    ),
                    .downloaded,
                    mine: true
                )
                Hairline()
                shelf(shotBook("Persuasion", id: "theirs", shared: true, mine: false, author: "Austen"), .failed("offline"), mine: false)
                Hairline()
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("offline-shelf")
            VStack(alignment: .leading, spacing: 0) {
                DownloadAllRow(summary: OfflineSummary(downloaded: 0, total: 12, running: false), identifier: "download-all-idle") {}
                DownloadAllRow(summary: OfflineSummary(downloaded: 3, total: 12, running: true), identifier: "download-all-running") {}
                DownloadAllRow(summary: OfflineSummary(downloaded: 12, total: 12, running: false), identifier: "download-all-done") {}
            }
            .padding(.horizontal, 16)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("offline-settings")
        }
        .background(mewo.ground)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("offline-preview")
    }

    private func shelf(_ book: BookRecord, _ state: OfflineState, mine: Bool) -> some View {
        ShelfBookRow(
            book: book,
            cover: nil,
            offline: state,
            onOpen: {},
            onDownload: { _ in },
            onShare: mine ? {} : nil,
            onDelete: mine ? {} : nil
        )
    }

    private func shotBook(
        _ title: String,
        id: String,
        shared: Bool,
        mine: Bool = true,
        author: String = "Jane Austen"
    ) -> BookRecord {
        BookRecord(
            id: id,
            title: title,
            author: author,
            handle: "austen",
            importedAt: 1,
            postCount: 120,
            progressIndex: 40,
            mine: mine,
            shared: shared,
            ownerUsername: mine ? nil : "ada"
        )
    }
}
