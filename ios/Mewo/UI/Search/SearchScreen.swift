import SwiftUI

/// Search state. The query updates at once; each change cancels the last
/// search, and only the newest search may publish. Hits show before likes
/// and comment counts load.
@Observable
@MainActor
final class SearchViewModel {
    var query = ""
    var result = SearchResult()
    var likes: [String: Set<String>] = [:]
    var comments: [String: CommentIndex] = [:]
    private let library: LibraryStore
    /// The running search and social load. Tests await it.
    @ObservationIgnored private(set) var searchTask: Task<Void, Never>?
    /// Bumped per search, so a search started for an older query (even the
    /// same text typed again) cannot publish.
    @ObservationIgnored private var generation = 0
    /// Books whose likes and comments loaded since the screen last appeared.
    @ObservationIgnored private var hydrated: Set<String> = []
    /// A completed toggle wins over an older likes load for that book.
    @ObservationIgnored private var likeEdits: [String: Int] = [:]
    /// Replay edited threads over an incoming index without losing other counts.
    @ObservationIgnored private var commentEdits: [String: [String: [PostComment]]] = [:]

    init(library: LibraryStore) { self.library = library }

    func setQuery(_ query: String) {
        if query == self.query { return }
        self.query = query
        run()
    }

    /// Searches again and reloads likes and comments, which may have changed
    /// in the reader. Called when the screen appears.
    func refresh() {
        hydrated.removeAll()
        run()
    }

    private func run() {
        searchTask?.cancel()
        generation += 1
        commentEdits.removeAll()
        let token = generation
        let query = self.query
        if query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            searchTask = nil
            result = SearchResult()
            return
        }
        searchTask = Task {
            guard let hits = try? await library.search(query: query), token == generation else { return }
            result = hits
            await hydrate(hits, token: token)
        }
    }

    /// Loads likes and comment counts for hit books not loaded yet, one book
    /// at a time, publishing each as it lands. A response that returns after
    /// a newer search or refresh started is dropped, even if the store ignored
    /// cancellation. Newer likes and edited threads keep the user's changes.
    private func hydrate(_ hits: SearchResult, token: Int) async {
        var seen = Set<String>()
        for hit in hits.posts where seen.insert(hit.book.id).inserted && !hydrated.contains(hit.book.id) {
            let id = hit.book.id
            let likeEdit = likeEdits[id, default: 0]
            let liked = try? await library.likes(id: id)
            guard token == generation else { return }
            guard let liked else { continue }
            var index = (try? await library.commentIndex(id: id)) ?? CommentIndex()
            guard token == generation else { return }
            let likesFresh = likeEdits[id, default: 0] == likeEdit
            for (postId, thread) in commentEdits[id] ?? [:] {
                index = index.afterThread(postId: postId, comments: thread)
            }
            if likesFresh { likes[id] = liked }
            comments[id] = index
            if likesFresh { hydrated.insert(id) }
        }
    }

    /// Returns the request so tests can await it.
    @discardableResult
    func toggleLike(bookId: String, postId: String) -> Task<Void, Never> {
        return Task {
            if let next = try? await library.toggleLike(id: bookId, postId: postId) {
                likeEdits[bookId, default: 0] += 1
                likes[bookId] = next
            }
        }
    }

    func onCommentsChanged(bookId: String, postId: String, comments: [PostComment]) {
        commentEdits[bookId, default: [:]][postId] = comments
        let current = self.comments[bookId] ?? CommentIndex()
        self.comments[bookId] = current.afterThread(postId: postId, comments: comments)
    }
}

struct SearchScreen: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Bindable var chrome: HideOnScrollState
    var onOpenBook: (String) -> Void
    var onOpenPost: (String, String) -> Void
    var onOpenAccount: () -> Void
    var onOpenProfile: (String, String) -> Void
    @State private var model: SearchViewModel
    @State private var commentTarget: CommentTarget?
    @State private var shareText: String?
    @State private var lastOffset: CGFloat = 0
    @FocusState private var focused: Bool

    init(env: AppEnvironment, chrome: HideOnScrollState, onOpenBook: @escaping (String) -> Void, onOpenPost: @escaping (String, String) -> Void, onOpenAccount: @escaping () -> Void, onOpenProfile: @escaping (String, String) -> Void) {
        self.chrome = chrome
        self.onOpenBook = onOpenBook
        self.onOpenPost = onOpenPost
        self.onOpenAccount = onOpenAccount
        self.onOpenProfile = onOpenProfile
        _model = State(initialValue: SearchViewModel(library: env.library))
    }

    var body: some View {
        VStack(spacing: 0) {
            MewoAppBar(leading: { ProfileButton(onClick: onOpenAccount) }, content: {
                TextField("Search", text: Binding(
                    get: { model.query },
                    set: { model.setQuery($0) }
                ))
                .textFieldStyle(.plain)
                .font(MewoFont.regular(17))
                .foregroundStyle(mewo.ink)
                .focused($focused)
                .padding(.trailing, 16)
                .accessibilityLabel("Search books and posts")
                .accessibilityIdentifier("search-field")
            })
            results
        }
        .background(mewo.ground)
        .onAppear {
            focused = true
            model.refresh()
        }
        .onChange(of: model.query) { _, q in
            chrome.showWhenIdle(q.isEmpty || (model.result.books.isEmpty && model.result.posts.isEmpty))
        }
        .sheet(item: $commentTarget) { target in
            CommentSheet(
                target: target,
                publicNotes: app.backend.kind == .hosted,
                onThreadChanged: { postId, comments in
                    model.onCommentsChanged(bookId: target.book.id, postId: postId, comments: comments)
                },
                onDismiss: { commentTarget = nil },
                onOpenProfile: { handle, name in
                    commentTarget = nil
                    onOpenProfile(handle, name)
                }
            )
        }
        .sheet(isPresented: Binding(get: { shareText != nil }, set: { if !$0 { shareText = nil } })) {
            if let shareText { ShareSheet(text: shareText) }
        }
    }

    @ViewBuilder
    private var results: some View {
        if model.query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            Color.clear
        } else if model.result.books.isEmpty && model.result.posts.isEmpty {
            Text("No matches")
                .font(MewoFont.bold(22))
                .foregroundStyle(mewo.ink)
                .padding(.horizontal, 28)
                .padding(.vertical, 48)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    GeometryReader { geo in
                        Color.clear.preference(key: ScrollOffsetKey.self, value: -geo.frame(in: .named("search")).minY)
                    }
                    .frame(height: 0)
                    if !model.result.books.isEmpty {
                        section("Books")
                        ForEach(model.result.books) { book in
                            Button { onOpenBook(book.id) } label: {
                                HStack(spacing: 12) {
                                    Avatar(name: book.author, cover: app.library.coverFile(id: book.id), size: 40)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(book.title).font(MewoFont.bold(15)).foregroundStyle(mewo.ink)
                                        Text("@\(book.handle)").font(MewoFont.regular(13)).foregroundStyle(mewo.mute)
                                    }
                                    Spacer()
                                }
                                .padding(.horizontal, 16)
                                .padding(.vertical, 12)
                            }
                            .buttonStyle(.plain)
                            Hairline()
                        }
                    }
                    if !model.result.posts.isEmpty {
                        section("Posts")
                        ForEach(model.result.posts) { hit in
                            TimelinePost(
                                post: hit.post,
                                author: hit.book.author,
                                handle: hit.book.handle,
                                cover: app.library.coverFile(id: hit.book.id),
                                liked: model.likes[hit.book.id]?.contains(hit.post.id) == true,
                                commentCount: model.comments[hit.book.id]?.count(hit.post.id) ?? 0,
                                commented: model.comments[hit.book.id]?.commented(hit.post.id) == true,
                                onComment: { commentTarget = CommentTarget(book: hit.book, post: hit.post) },
                                onLike: { model.toggleLike(bookId: hit.book.id, postId: hit.post.id) },
                                onRepost: { shareText = quoteLine(author: hit.book.author, title: hit.book.title, text: hit.post.text) },
                                onShare: { shareText = hit.post.text },
                                onOpen: { onOpenPost(hit.book.id, hit.post.id) }
                            )
                        }
                    }
                }
            }
            .coordinateSpace(name: "search")
            .onPreferenceChange(ScrollOffsetKey.self) { offset in
                chrome.ingest(delta: offset - lastOffset)
                lastOffset = offset
                chrome.bindTop(offset)
            }
        }
    }

    private func section(_ text: String) -> some View {
        Text(text)
            .font(MewoFont.regular(13))
            .foregroundStyle(mewo.mute)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
    }

}
