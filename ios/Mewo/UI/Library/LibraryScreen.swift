import SwiftUI
import UniformTypeIdentifiers

@Observable
@MainActor
final class LibraryViewModel {
    var busy = false
    var error: String?
    var notice: String?
    private let env: AppEnvironment

    init(env: AppEnvironment) {
        self.env = env
    }

    func start() {
        Task { await refresh() }
    }

    /// Same load the screen uses on launch. A pull while one is running waits on it.
    func refresh() async {
        try? await env.library.load()
    }

    func importFile(_ url: URL) {
        runWork {
            let book = try await self.env.library.importFromFile(url)
            if self.env.backend.kind == .hosted, let message = privateUploadNotice(for: book) {
                self.notice = message
            }
        }
    }

    func importSample() {
        runWork { _ = try await self.env.library.importSample() }
    }

    func delete(id: String) {
        Task {
            do {
                try await env.library.delete(id: id)
            } catch {
                self.error = error.localizedDescription
            }
        }
    }

    func setShared(id: String, shared: Bool) {
        Task {
            do {
                try await env.hostedLibrary.setShared(id: id, shared: shared)
            } catch {
                self.error = error.localizedDescription
            }
        }
    }

    func dismissBanner() {
        if error != nil || notice != nil {
            error = nil
            notice = nil
            return
        }
        env.library.dismissLoadError()
    }

    private func runWork(_ block: @escaping () async throws -> Void) {
        Task {
            busy = true
            error = nil
            notice = nil
            do {
                try await block()
            } catch {
                self.error = error.localizedDescription
            }
            busy = false
        }
    }
}

struct LibraryScreen: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Bindable var chrome: HideOnScrollState
    var onOpenBook: (String) -> Void
    var onOpenAccount: () -> Void
    var onBrowseCatalog: () -> Void
    var showsBar: Bool
    @State private var model: LibraryViewModel
    @State private var picking = false
    @State private var pendingDelete: BookRecord?
    @State private var lastOffset: CGFloat = 0

    init(env: AppEnvironment, chrome: HideOnScrollState, onOpenBook: @escaping (String) -> Void, onOpenAccount: @escaping () -> Void, onBrowseCatalog: @escaping () -> Void = {}, showsBar: Bool = true) {
        self.chrome = chrome
        self.onOpenBook = onOpenBook
        self.onOpenAccount = onOpenAccount
        self.onBrowseCatalog = onBrowseCatalog
        self.showsBar = showsBar
        _model = State(initialValue: LibraryViewModel(env: env))
    }

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            VStack(spacing: 0) {
                if showsBar {
                    MewoAppBar(visible: chrome.visible, leading: { ProfileButton(onClick: onOpenAccount) }, center: { BrandMark() })
                }
                GeometryReader { geo in
                    ScrollView {
                        shelfBody(minHeight: geo.size.height)
                    }
                    .coordinateSpace(name: "library")
                    .refreshable { await model.refresh() }
                    .scrollBounceBehavior(.always)
                    .scrollIndicators(.hidden)
                    .accessibilityIdentifier("library-scroll")
                }
            }
            AddBookButton(
                canRequest: app.catalog.access == .on,
                identifier: "add-epub",
                accessibilityLabel: app.catalog.access == .on ? "Add a book" : "Add an EPUB",
                onUpload: { picking = true },
                onBrowseCatalog: onBrowseCatalog
            ) {
                Image(systemName: "plus")
                    .font(.system(size: 22, weight: .semibold))
                    .foregroundStyle(mewo.onAccent)
                    .frame(width: 56, height: 56)
                    .background(mewo.accent, in: Circle())
            }
            .padding(.trailing, 16)
            .padding(.bottom, 16)
            if let message = model.error ?? model.notice ?? loadNotice {
                banner(message)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 80)
                    .frame(maxWidth: .infinity, alignment: .bottom)
            }
            if model.busy {
                VStack(spacing: 8) {
                    Text("Uploading…")
                        .font(MewoFont.bold(15))
                        .foregroundStyle(mewo.ink)
                    ProgressView().tint(mewo.accent)
                }
                .padding(.bottom, 24)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Uploading…")
            }
        }
        .background(mewo.ground)
        .fileImporter(isPresented: $picking, allowedContentTypes: epubTypes, allowsMultipleSelection: false) { result in
            if case .success(let urls) = result, let url = urls.first {
                model.importFile(url)
            }
        }
        .alert(pendingDelete.map { "Remove \($0.title)?" } ?? "Remove?", isPresented: Binding(
            get: { pendingDelete != nil },
            set: { if !$0 { pendingDelete = nil } }
        )) {
            Button("Keep", role: .cancel) { pendingDelete = nil }
            Button("Remove", role: .destructive) {
                if let book = pendingDelete { model.delete(id: book.id) }
                pendingDelete = nil
            }
        } message: {
            if let book = pendingDelete {
                Text(removeDetail(book))
            }
        }
        .onAppear {
            model.start()
            chrome.showWhenIdle(shelf.isEmpty)
        }
        .onChange(of: shelf.count) { _, count in
            chrome.showWhenIdle(count == 0)
        }
    }

    /// Live shelf. Empty copy waits until a fetch succeeds with nothing saved.
    private var load: LibraryLoad {
        app.backend.kind == .hosted ? app.hostedLibrary.loadState : app.localLibrary.loadState
    }

    private var shelf: [BookRecord] { load.books }

    private var loadNotice: String? {
        if case .failed(let message, .some) = load { return message }
        return nil
    }

    @ViewBuilder
    private func shelfBody(minHeight: CGFloat) -> some View {
        if showsQuiet {
            quiet(minHeight: minHeight) {
                if case .failed(let message, nil) = load {
                    unavailable(message)
                } else if load.showEmpty && !model.busy {
                    emptyContent
                } else if !model.busy {
                    ProgressView()
                        .tint(mewo.accent)
                        .accessibilityIdentifier("library-loading")
                        .accessibilityLabel("Loading your shelf")
                }
            }
        } else {
            list
        }
    }

    private var showsQuiet: Bool {
        if case .failed(_, nil) = load { return true }
        if case .loading = load { return true }
        return load.showEmpty
    }

    private func quiet<Content: View>(minHeight: CGFloat, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Spacer()
            content()
            Spacer()
        }
        .frame(maxWidth: .infinity, minHeight: minHeight, alignment: .leading)
        .padding(.horizontal, 28)
    }

    private func unavailable(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Couldn't load")
                .font(MewoFont.bold(22))
                .foregroundStyle(mewo.ink)
            Spacer().frame(height: 12)
            Text(message)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.mute)
            Button("Try again") { Task { await model.refresh() } }
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.accent)
                .buttonStyle(.plain)
                .padding(.top, 8)
                .accessibilityIdentifier("library-retry")
        }
        .accessibilityIdentifier("library-unavailable")
    }

    private var emptyContent: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("No books")
                .font(MewoFont.bold(22))
                .foregroundStyle(mewo.ink)
            Spacer().frame(height: 20)
            AddBookButton(
                canRequest: app.catalog.access == .on,
                identifier: "add-book-empty",
                onUpload: { picking = true },
                onBrowseCatalog: onBrowseCatalog
            ) {
                Text("Add a book")
                    .font(MewoFont.bold(14))
                    .foregroundStyle(mewo.onAccent)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(mewo.accent, in: Capsule())
            }
            Button("Open a sample") { model.importSample() }
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.accent)
                .buttonStyle(.plain)
                .padding(.top, 8)
                .accessibilityIdentifier("open-sample")
        }
    }

    private var hosted: Bool { app.backend.kind == .hosted }
    private var reading: [BookRecord] { shelf.filter { !$0.isFinished } }
    private var finished: [BookRecord] { shelf.filter(\.isFinished) }

    private var list: some View {
        LazyVStack(spacing: 0) {
            offsetTracker
            if hosted {
                let groups = hostedShelfGroups(shelf)
                if !groups.mine.isEmpty {
                    sectionLabel("My books")
                    rows(groups.mine)
                }
                if !groups.sharedWithMe.isEmpty {
                    sectionLabel("Shared with me")
                    rows(groups.sharedWithMe)
                }
                if !groups.finished.isEmpty {
                    sectionLabel("Finished")
                    rows(groups.finished)
                }
            } else {
                rows(reading)
                if !finished.isEmpty {
                    sectionLabel("Finished")
                    rows(finished)
                }
            }
        }
        .padding(.bottom, 96)
    }

    private func rows(_ books: [BookRecord]) -> some View {
        ForEach(books) { book in
            bookRow(book)
            Hairline()
        }
    }

    private func banner(_ message: String) -> some View {
        HStack(alignment: .center, spacing: 8) {
            Text(message)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.ink)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button {
                model.dismissBanner()
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(mewo.mute)
                    .frame(width: 40, height: 40)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Dismiss message")
        }
        .padding(.leading, 16)
        .padding(.trailing, 8)
        .padding(.vertical, 8)
        .background(mewo.hover)
        .overlay(Rectangle().stroke(mewo.line, lineWidth: 1))
        .accessibilityIdentifier("library-notice")
    }

    private func removeDetail(_ book: BookRecord) -> String {
        if !hosted { return "Removes it from this phone." }
        if book.shared || book.isSample { return "Removes it for everyone who can read it." }
        return "Removes it from your library."
    }

    private func sectionLabel(_ text: String) -> some View {
        Text(text)
            .font(MewoFont.regular(13))
            .foregroundStyle(mewo.mute)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
    }

    private var offsetTracker: some View {
        GeometryReader { geo in
            Color.clear.preference(key: ScrollOffsetKey.self, value: -geo.frame(in: .named("library")).minY)
        }
        .frame(height: 0)
        .onPreferenceChange(ScrollOffsetKey.self) { offset in
            chrome.ingest(delta: offset - lastOffset)
            lastOffset = offset
            chrome.bindTop(offset)
        }
    }

    private func bookRow(_ book: BookRecord) -> some View {
        ShelfBookRow(
            book: book,
            cover: app.library.coverFile(id: book.id),
            offline: hosted ? (app.hostedLibrary.offline[book.id] ?? .notDownloaded) : nil,
            onOpen: { onOpenBook(book.id) },
            onDownload: hosted ? { download in
                Task { await app.library.setDownloaded(id: book.id, on: download) }
            } : nil,
            onShare: hosted && book.mine ? {
                model.setShared(id: book.id, shared: !book.shared)
            } : nil,
            onDelete: book.mine ? { pendingDelete = book } : nil
        )
    }
}

struct ShelfBookRow: View {
    @Environment(\.mewo) private var mewo
    var book: BookRecord
    var cover: URL?
    var offline: OfflineState?
    var onOpen: () -> Void
    var onDownload: ((Bool) -> Void)?
    var onShare: (() -> Void)?
    var onDelete: (() -> Void)?

    var body: some View {
        HStack(alignment: .center, spacing: 0) {
            Button(action: onOpen) {
                HStack(alignment: .center, spacing: 12) {
                    Avatar(name: book.author, cover: cover, size: 40)
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            Text(book.title).font(MewoFont.bold(15)).lineLimit(1)
                            Text("@\(book.handle)").font(MewoFont.regular(13)).foregroundStyle(mewo.mute).lineLimit(1)
                        }
                        Text(statusLine(for: book))
                            .font(MewoFont.regular(15))
                            .foregroundStyle(mewo.ink)
                        if let credit = ownerCredit(book) {
                            Text(credit)
                                .font(MewoFont.regular(13))
                                .foregroundStyle(mewo.mute)
                                .lineLimit(1)
                        }
                    }
                    Spacer(minLength: 0)
                }
            }
            .buttonStyle(.plain)
            HStack(alignment: .center, spacing: 0) {
                if let offline, let onDownload {
                    downloadButton(offline, onDownload: onDownload)
                }
                if let onShare {
                    shelfAction(
                        symbol: book.shared ? "person.2.fill" : "lock",
                        label: book.shared ? "Make private" : "Share",
                        identifier: "share-\(book.id)"
                    ) { onShare() }
                }
                if let onDelete {
                    shelfAction(symbol: "trash", label: "Remove \(book.title)", identifier: "remove-\(book.id)") {
                        onDelete()
                    }
                }
            }
        }
        .padding(.leading, 16)
        .padding(.trailing, 4)
        .padding(.vertical, 8)
        .accessibilityIdentifier("book-\(book.id)")
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder
    private func downloadButton(_ state: OfflineState, onDownload: @escaping (Bool) -> Void) -> some View {
        switch state {
        case .notDownloaded:
            shelfAction(symbol: "arrow.down.circle", label: "Download", identifier: "download-\(book.id)") {
                onDownload(true)
            }
        case .downloading(let fraction):
            Button { onDownload(false) } label: {
                DownloadRing(fraction: fraction)
                    .frame(width: 40, height: 40)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(downloadingLabel(fraction))
            .accessibilityIdentifier("download-\(book.id)")
        case .downloaded:
            shelfAction(symbol: "arrow.down.circle.fill", label: "Remove download", identifier: "download-\(book.id)") {
                onDownload(false)
            }
        case .failed:
            Button { onDownload(true) } label: {
                Image(systemName: "exclamationmark.circle")
                    .font(.system(size: 17))
                    .foregroundStyle(mewo.like)
                    .frame(width: 40, height: 40, alignment: .center)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Download failed, try again")
            .accessibilityIdentifier("download-\(book.id)")
        }
    }

    /// Same 40pt cell as remove, so the icons share one vertical axis.
    private func shelfAction(symbol: String, label: String, identifier: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 17))
                .foregroundStyle(mewo.mute)
                .frame(width: 40, height: 40, alignment: .center)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .accessibilityIdentifier(identifier)
    }
}

/// 20pt ring. A nil fraction spins; a value is a trimmed circle because `ProgressView` ignores it.
struct DownloadRing: View {
    @Environment(\.mewo) private var mewo
    var fraction: Double?

    var body: some View {
        Group {
            if let fraction {
                ZStack {
                    Circle().stroke(mewo.line, lineWidth: 2)
                    Circle()
                        .trim(from: 0, to: min(1, max(0, fraction)))
                        .stroke(mewo.mute, style: StrokeStyle(lineWidth: 2, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                }
            } else {
                ProgressView()
                    .controlSize(.small)
                    .tint(mewo.mute)
            }
        }
        .frame(width: 20, height: 20)
    }
}

/// Spoken name for the shelf ring. A missing length has no percent.
func downloadingLabel(_ fraction: Double?) -> String {
    guard let fraction else { return "Downloading" }
    let pct = min(100, max(0, Int((fraction * 100).rounded())))
    return "Downloading, \(pct)%"
}

struct ScrollOffsetKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

let epubTypes: [UTType] = [
    UTType(filenameExtension: "epub") ?? .data,
    UTType(exportedAs: "com.adobe.epub+zip"),
]
