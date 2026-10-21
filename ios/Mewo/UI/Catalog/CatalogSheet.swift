import SwiftUI

struct CatalogSheet: View {
    @Environment(\.mewo) private var mewo
    @Bindable var model: CatalogSheetModel
    var onOpenBook: (String) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            header
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    hero
                    statusBlock
                    aboutBlock
                }
                .padding(.bottom, 32)
            }
        }
        .background(mewo.ground)
        .accessibilityIdentifier("catalog-sheet")
    }

    private var header: some View {
        HStack(spacing: 8) {
            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(mewo.ink)
                    .frame(width: 44, height: 44)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Close")
            Spacer(minLength: 0)
        }
        .padding(.leading, 4)
    }

    private var hero: some View {
        HStack(alignment: .top, spacing: 16) {
            CatalogCover(title: displayTitle, coverId: displayCover, width: 72, height: 108)
            VStack(alignment: .leading, spacing: 4) {
                Text(displayTitle)
                    .font(MewoFont.bold(20))
                    .foregroundStyle(mewo.ink)
                Text(heroMeta)
                    .font(MewoFont.regular(15))
                    .foregroundStyle(mewo.mute)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
    }

    private var statusBlock: some View {
        VStack(alignment: .leading, spacing: 12) {
            statusBody
            if let line = model.alsoRequested {
                Text(line)
                    .font(MewoFont.regular(15))
                    .foregroundStyle(mewo.mute)
            }
            if let notice = model.notice {
                Text(notice)
                    .font(MewoFont.regular(15))
                    .foregroundStyle(mewo.ink)
                    .accessibilityIdentifier("catalog-sheet-notice")
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 24)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Download status")
    }

    @ViewBuilder
    private var statusBody: some View {
        switch model.status {
        case .live(_, let line, let progress):
            Text(line)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.ink)
                .accessibilityIdentifier("catalog-status")
            if let progress {
                ProgressView(value: Double(progress), total: 100)
                    .tint(mewo.accent)
                    .accessibilityLabel(line)
            }
            outlined("Cancel request", identifier: "catalog-cancel", enabled: !model.acting) {
                Task { await model.cancel() }
            }
        case .failed(let line, let note):
            Text(line)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.ink)
                .accessibilityIdentifier("catalog-status")
            filled("Retry", identifier: "catalog-retry", enabled: !model.acting && !model.atCap) {
                Task { await model.retry() }
            }
            noteText(note)
        case .available(let line, let bookId):
            Text(line)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.ink)
                .accessibilityIdentifier("catalog-status")
            filled("Open book", identifier: "catalog-open", enabled: bookId != nil) {
                if let bookId { onOpenBook(bookId) }
            }
        case .canceled(let note):
            filled("Request again", identifier: "catalog-again", enabled: !model.acting && !model.atCap) {
                Task { await model.retry() }
            }
            noteText(note)
        case .requestable(let note, let edition):
            filled("Request", identifier: "catalog-request", enabled: !model.acting && !model.atCap) {
                Task { await model.request() }
            }
            noteText(note)
            noteText(edition)
        }
    }

    @ViewBuilder
    private var aboutBlock: some View {
        switch model.about {
        case .loading:
            VStack(alignment: .leading, spacing: 8) {
                bone
                bone.frame(maxWidth: 220)
                bone
            }
            .padding(.horizontal, 16)
            .padding(.top, 28)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Loading details…")
        case .failed(let message):
            retryLine(message)
        case .missing:
            retryLine("Couldn't load details from Open Library.")
        case .ready(let about):
            VStack(alignment: .leading, spacing: 24) {
                if let blurb = about.blurb {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(blurb)
                            .font(MewoFont.regular(15))
                            .foregroundStyle(mewo.ink)
                            .lineLimit(model.expanded ? nil : 6)
                        if model.blurbOverflow || model.expanded {
                            Button(model.expanded ? "Less" : "More") { model.expanded.toggle() }
                                .font(MewoFont.regular(15))
                                .foregroundStyle(mewo.accent)
                                .buttonStyle(.plain)
                        }
                    }
                }
                if !about.series.isEmpty || !about.awards.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        if !about.series.isEmpty {
                            labeled("Series", about.series.joined(separator: " · "))
                        }
                        if !about.awards.isEmpty {
                            labeled("Awards", about.awards.joined(separator: " · "))
                        }
                    }
                }
                if !about.subjects.isEmpty {
                    Text(about.subjects)
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.mute)
                }
                if !about.stats.isEmpty {
                    Text(about.stats)
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.ink)
                }
                if let url = about.libraryURL {
                    Link(destination: url) {
                        HStack(spacing: 6) {
                            Text("View on Open Library")
                            Image(systemName: "arrow.up.right")
                                .font(.system(size: 14, weight: .semibold))
                        }
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.accent)
                    }
                    .accessibilityIdentifier("catalog-open-library")
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 28)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var displayTitle: String { model.detail?.info?.title ?? model.work.title }

    private var displayCover: Int? { model.detail?.info?.coverId ?? model.work.coverId }

    private var heroMeta: String {
        let author: String
        if let authors = model.detail?.info?.authors, !authors.isEmpty {
            author = authors.joined(separator: ", ")
        } else {
            author = model.work.author
        }
        let year = model.detail?.info?.year ?? model.work.year
        if let year { return "\(author) · \(year)" }
        return author
    }

    private func filled(_ title: String, identifier: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .buttonStyle(.borderedProminent)
            .buttonBorderShape(.capsule)
            .controlSize(.large)
            .tint(mewo.accent)
            .disabled(!enabled)
            .frame(maxWidth: .infinity)
            .accessibilityIdentifier(identifier)
    }

    private func outlined(_ title: String, identifier: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(title, role: .destructive, action: action)
            .buttonStyle(.bordered)
            .buttonBorderShape(.capsule)
            .controlSize(.large)
            .disabled(!enabled)
            .frame(maxWidth: .infinity)
            .accessibilityIdentifier(identifier)
    }

    @ViewBuilder
    private func noteText(_ text: String?) -> some View {
        if let text {
            Text(text)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.mute)
        }
    }

    private func labeled(_ title: String, _ value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            Text(title)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.mute)
                .frame(width: 76, alignment: .leading)
            Text(value)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.ink)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func retryLine(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(message)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.mute)
            Button("Try again") { model.reload() }
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.accent)
                .buttonStyle(.plain)
        }
        .padding(.horizontal, 16)
        .padding(.top, 28)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var bone: some View {
        RoundedRectangle(cornerRadius: 4)
            .fill(mewo.hover)
            .frame(height: 14)
            .frame(maxWidth: .infinity)
    }
}
