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
        Task { await load() }
    }

    func load() async {
        do {
            try await env.library.load()
        } catch {
            self.error = error.localizedDescription
        }
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
        error = nil
        notice = nil
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
    @State private var model: LibraryViewModel
    @State private var picking = false
    @State private var pendingDelete: BookRecord?
    @State private var lastOffset: CGFloat = 0

    init(env: AppEnvironment, chrome: HideOnScrollState, onOpenBook: @escaping (String) -> Void, onOpenAccount: @escaping () -> Void) {
        self.chrome = chrome
        self.onOpenBook = onOpenBook
        self.onOpenAccount = onOpenAccount
        _model = State(initialValue: LibraryViewModel(env: env))
    }

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            VStack(spacing: 0) {
                MewoAppBar(visible: chrome.visible, leading: { ProfileButton(onClick: onOpenAccount) }, center: { BrandMark() })
                if shelf.isEmpty && !model.busy {
                    empty
                } else {
                    list
                }
            }
            Button {
                picking = true
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 22, weight: .semibold))
                    .foregroundStyle(mewo.onAccent)
                    .frame(width: 56, height: 56)
                    .background(mewo.accent, in: Circle())
            }
            .buttonStyle(.plain)
            .padding(.trailing, 16)
            .padding(.bottom, 16)
            .accessibilityLabel("Add an EPUB")
            .accessibilityIdentifier("add-epub")
            if let message = model.error ?? model.notice {
                banner(message)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 80)
                    .frame(maxWidth: .infinity, alignment: .bottom)
            }
            if model.busy {
                ProgressView().tint(mewo.accent).padding(.bottom, 24).frame(maxWidth: .infinity, alignment: .center)
            }
        }
        .background(mewo.ground)
        .fileImporter(isPresented: $picking, allowedContentTypes: epubTypes, allowsMultipleSelection: false) { result in
            if case .success(let urls) = result, let url = urls.first {
                model.importFile(url)
            }
        }
        .alert("Remove from your library?", isPresented: Binding(
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

    /// Live shelf, not the view-model snapshot. Progress saved in the reader
    /// has to move a book under Finished when you come back.
    private var shelf: [BookRecord] {
        app.backend.kind == .hosted ? app.hostedLibrary.books : app.localLibrary.books
    }

    private var empty: some View {
        VStack(alignment: .leading, spacing: 0) {
            Spacer()
            Text("Your timeline is empty")
                .font(MewoFont.bold(22))
                .foregroundStyle(mewo.ink)
            Spacer().frame(height: 8)
            Text(hosted
                 ? "Drop in an EPUB and the author starts posting, one paragraph at a time. It stays private until you share it."
                 : "Drop in an EPUB and the author starts posting, one paragraph at a time.")
                .font(MewoFont.regular(17))
                .foregroundStyle(mewo.mute)
            Spacer().frame(height: 20)
            Button {
                picking = true
            } label: {
                Text("Add a book")
                    .font(MewoFont.regular(15))
                    .foregroundStyle(mewo.onAccent)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(mewo.accent, in: Capsule())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("add-book-empty")
            Button("Open a sample") { model.importSample() }
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.accent)
                .buttonStyle(.plain)
                .padding(.top, 8)
                .accessibilityIdentifier("open-sample")
            Spacer()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 28)
    }

    private var hosted: Bool { app.backend.kind == .hosted }
    private var reading: [BookRecord] { shelf.filter { !$0.isFinished } }
    private var finished: [BookRecord] { shelf.filter(\.isFinished) }

    private var list: some View {
        ScrollView {
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
        .scrollIndicators(.hidden)
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
        if !hosted {
            return "This removes \(book.title), plus likes, notes, and where you left off. You can add the EPUB again."
        }
        if book.shared || book.isSample {
            return "This removes \(book.title) for everyone who can read it, plus likes, notes, and reading place."
        }
        return "This removes \(book.title) from your library, plus likes, notes, and reading place. It is private, so nobody else can read it."
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
        HStack(alignment: .top, spacing: 8) {
            Button {
                onOpenBook(book.id)
            } label: {
                HStack(alignment: .top, spacing: 12) {
                    Avatar(name: book.author, cover: app.library.coverFile(id: book.id), size: 40)
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
            if hosted && book.mine {
                shareControl(book)
            }
            if book.mine {
                Button {
                    pendingDelete = book
                } label: {
                    Image(systemName: "trash")
                        .foregroundStyle(mewo.mute)
                        .frame(width: 40, height: 40)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Remove \(book.title)")
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .accessibilityIdentifier("book-\(book.id)")
        .accessibilityElement(children: .contain)
    }

    private func shareControl(_ book: BookRecord) -> some View {
        Toggle(isOn: Binding(
            get: { book.shared },
            set: { model.setShared(id: book.id, shared: $0) }
        )) {
            Text(book.shared ? "Shared" : "Share")
                .font(MewoFont.regular(13))
                .foregroundStyle(mewo.mute)
        }
        .tint(mewo.accent)
        .fixedSize()
        .accessibilityLabel(book.shared ? "Stop sharing \(book.title)" : "Share \(book.title)")
        .accessibilityIdentifier("share-\(book.id)")
    }
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
