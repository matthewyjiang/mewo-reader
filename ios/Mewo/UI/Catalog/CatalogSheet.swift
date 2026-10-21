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
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 24, weight: .regular))
                        .foregroundStyle(mewo.ink)
                        .frame(width: 48, height: 48)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close")
                Spacer(minLength: 0)
            }
            .padding(.leading, 4)
            .frame(height: 56)
            Hairline()
        }
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
        .padding(.top, 16)
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
                    .foregroundStyle(mewo.like)
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
            statusLine(line)
            if let progress {
                progressBar(progress)
                    .accessibilityLabel(line)
            }
            outlined("Cancel request", identifier: "catalog-cancel", enabled: !model.acting) {
                Task { await model.cancel() }
            }
        case .failed(let line, let note):
            statusLine(line)
            filled("Retry", identifier: "catalog-retry", enabled: !model.acting && !model.atCap) {
                Task { await model.retry() }
            }
            noteText(note)
        case .available(let line, let bookId):
            statusLine(line)
            filled("Open book", identifier: "catalog-open", enabled: bookId != nil) {
                if let bookId { onOpenBook(bookId) }
            }
        case .canceled(let note):
            filled("Request again", identifier: "catalog-again", enabled: !model.acting && !model.atCap) {
                Task { await model.retry() }
            }
            noteText(note)
        case .requestAgain(let note):
            filled("Request again", identifier: "catalog-again", enabled: !model.acting && !model.atCap) {
                Task { await model.request() }
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
            GeometryReader { geo in
                VStack(alignment: .leading, spacing: 10) {
                    bone.frame(width: geo.size.width)
                    bone.frame(width: geo.size.width * 0.64)
                    bone.frame(width: geo.size.width)
                }
            }
            .frame(height: 62)
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
                            .font(MewoFont.regular(17))
                            .foregroundStyle(mewo.ink)
                            .lineLimit(model.expanded ? nil : 6)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background {
                                GeometryReader { geo in
                                    Color.clear
                                        .onAppear { model.noteBlurbWidth(geo.size.width) }
                                        .onChange(of: geo.size.width) { _, width in
                                            model.noteBlurbWidth(width)
                                        }
                                }
                            }
                        if model.blurbOverflow || model.expanded {
                            textButton(model.expanded ? "Less" : "More") { model.expanded.toggle() }
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
                                .font(MewoFont.bold(15))
                            Image(systemName: "arrow.up.right.square")
                                .font(.system(size: 16, weight: .regular))
                        }
                        .foregroundStyle(mewo.accent)
                        .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                    }
                    .accessibilityIdentifier("catalog-open-library")
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 28)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var displayTitle: String { model.work.title }

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

    private func statusLine(_ line: String) -> some View {
        Text(line)
            .font(MewoFont.regular(17))
            .foregroundStyle(mewo.ink)
            .accessibilityIdentifier("catalog-status")
    }

    private func progressBar(_ value: Int) -> some View {
        let fraction = CGFloat(min(max(value, 0), 100)) / 100
        return GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule().fill(mewo.line)
                Capsule()
                    .fill(mewo.accent)
                    .frame(width: max(0, geo.size.width * fraction))
            }
        }
        .frame(height: 4)
    }

    private func filled(_ title: String, identifier: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(MewoFont.bold(15))
                .foregroundStyle(mewo.onAccent)
                .frame(maxWidth: .infinity, minHeight: 44)
        }
        .buttonStyle(.plain)
        .background(mewo.accent, in: Capsule())
        .opacity(enabled ? 1 : 0.4)
        .disabled(!enabled)
        .accessibilityIdentifier(identifier)
    }

    private func outlined(_ title: String, identifier: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(MewoFont.bold(15))
                .foregroundStyle(mewo.like)
                .frame(maxWidth: .infinity, minHeight: 44)
        }
        .buttonStyle(.plain)
        .background {
            Capsule().strokeBorder(mewo.line, lineWidth: 1)
        }
        .opacity(enabled ? 1 : 0.4)
        .disabled(!enabled)
        .accessibilityIdentifier(identifier)
    }

    private func textButton(_ title: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(MewoFont.bold(15))
                .foregroundStyle(mewo.accent)
                .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
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
                .font(MewoFont.regular(17))
                .foregroundStyle(mewo.mute)
                .frame(width: 76, alignment: .leading)
            Text(value)
                .font(MewoFont.regular(17))
                .foregroundStyle(mewo.ink)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func retryLine(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(message)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.mute)
            textButton("Try again") { model.reload() }
        }
        .padding(.horizontal, 16)
        .padding(.top, 28)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var bone: some View {
        RoundedRectangle(cornerRadius: 4)
            .fill(mewo.mute.opacity(0.16))
            .frame(height: 14)
    }
}
