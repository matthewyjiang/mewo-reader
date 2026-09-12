import SwiftUI
import UniformTypeIdentifiers

@Observable
@MainActor
final class LibraryViewModel {
    var books: [BookRecord] = []
    var busy = false
    var error: String?
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
            books = env.library.books
        } catch {
            self.error = error.localizedDescription
        }
    }

    func importFile(_ url: URL) {
        runWork { _ = try await self.env.library.importFromFile(url) }
    }

    func importSample() {
        runWork { _ = try await self.env.library.importSample() }
    }

    func delete(id: String) {
        Task {
            do {
                try await env.library.delete(id: id)
                books = env.library.books
            } catch {
                self.error = error.localizedDescription
            }
        }
    }

    private func runWork(_ block: @escaping () async throws -> Void) {
        Task {
            busy = true
            error = nil
            do {
                try await block()
                books = env.library.books
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
                if model.books.isEmpty && !model.busy {
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
                Text(app.backend.kind == .hosted
                     ? "This removes \(book.title) for everyone on this server, plus likes, notes, and reading place."
                     : "This removes \(book.title), plus likes, notes, and where you left off. You can add the EPUB again.")
            }
        }
        .onAppear {
            model.start()
            chrome.showWhenIdle(model.books.isEmpty)
        }
        .onChange(of: model.books.count) { _, count in
            chrome.showWhenIdle(count == 0)
        }
    }

    private var empty: some View {
        VStack(alignment: .leading, spacing: 0) {
            Spacer()
            Text("Your timeline is empty")
                .font(MewoFont.bold(22))
                .foregroundStyle(mewo.ink)
            Spacer().frame(height: 8)
            Text("Drop in an EPUB and the author starts posting, one paragraph at a time.")
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

    private var list: some View {
        ScrollView {
            LazyVStack(spacing: 0) {
                offsetTracker
                ForEach(model.books) { book in
                    bookRow(book)
                    Hairline()
                }
            }
            .padding(.bottom, 96)
        }
        .scrollIndicators(.hidden)
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
                }
                Spacer(minLength: 0)
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
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("book-\(book.id)")
        .accessibilityElement(children: .contain)
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
