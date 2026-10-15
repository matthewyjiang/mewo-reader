import SwiftUI

/// Search state. The query updates at once; each change cancels the last
/// scan, and only the newest scan may publish. Hits show before likes and
/// comment counts load. Each hit book's likes and comments live in a
/// `BookSocial` that outlives the query, so typing never restarts them.
@Observable
@MainActor
final class SearchViewModel {
    var query = ""
    var result = SearchResult()
    private var socials: [String: BookSocial] = [:]
    private let library: LibraryStore
    /// The running scan. Tests await it.
    @ObservationIgnored private(set) var searchTask: Task<Void, Never>?
    /// Bumped per scan, so a scan started for an older query (even the same
    /// text typed again) cannot publish.
    @ObservationIgnored private var generation = 0

    init(library: LibraryStore) { self.library = library }

    /// Likes and comment counts for a book, once a search has hit it.
    func social(for bookId: String) -> BookSocial? { socials[bookId] }

    func setQuery(_ query: String) {
        if query == self.query { return }
        self.query = query
        run()
    }

    /// Searches again and marks every book's likes and comments due, since
    /// they may have changed in the reader. Shown values stay until the
    /// reload lands. Called when the screen appears.
    func refresh() {
        for social in socials.values { social.invalidate() }
        run()
    }

    private func run() {
        searchTask?.cancel()
        generation += 1
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
            for hit in hits.posts { social(hit.book.id).load() }
        }
    }

    /// Returns the request so tests can await it.
    @discardableResult
    func toggleLike(bookId: String, postId: String) -> Task<Void, Never> {
        social(bookId).toggleLike(postId: postId)
    }

    func onCommentsChanged(bookId: String, postId: String, comments: [PostComment]) {
        social(bookId).threadChanged(postId: postId, comments: comments)
    }

    private func social(_ bookId: String) -> BookSocial {
        if let social = socials[bookId] { return social }
        let social = BookSocial(id: bookId, library: library)
        socials[bookId] = social
        return social
    }
}

/// Likes and comment counts for one book. Callers that overlap share one
/// load. Each field caches on success; on failure it keeps its last value
/// and stays due, so the next `load` retries it. A like toggle or thread
/// edit that lands during a load wins over that load's older answer.
@Observable
@MainActor
final class BookSocial {
    /// Nil until the first successful load or toggle.
    private(set) var likes: Set<String>?
    /// Nil until the first successful load or thread edit.
    private(set) var comments: CommentIndex?
    /// The running load. Tests await it.
    @ObservationIgnored private(set) var pending: Task<Void, Never>?
    @ObservationIgnored private var likesDue = true
    @ObservationIgnored private var commentsDue = true
    /// Bumped by `invalidate`, so a load started before it cannot publish.
    @ObservationIgnored private var epoch = 0
    /// Bumped per completed toggle. A load drops its likes if this moved.
    @ObservationIgnored private var likeEdits = 0
    /// Threads edited while `pending` runs, replayed over its index.
    @ObservationIgnored private var threadEdits: [String: [PostComment]] = [:]
    private let id: String
    private let library: LibraryStore

    init(id: String, library: LibraryStore) {
        self.id = id
        self.library = library
    }

    /// Loads the due fields, unless a load is running or nothing is due.
    func load() {
        guard pending == nil, likesDue || commentsDue else { return }
        let epoch = self.epoch
        let likeEdits = self.likeEdits
        let wantLikes = likesDue
        let wantComments = commentsDue
        pending = Task {
            var liked: Set<String>?
            var index: CommentIndex?
            if wantLikes { liked = try? await library.likes(id: id) }
            if wantComments { index = try? await library.commentIndex(id: id) }
            guard epoch == self.epoch else { return }
            pending = nil
            if let liked, likeEdits == self.likeEdits {
                likes = liked
                likesDue = false
            }
            if var index {
                for (postId, thread) in threadEdits {
                    index = index.afterThread(postId: postId, comments: thread)
                }
                comments = index
                commentsDue = false
            }
            threadEdits.removeAll()
        }
    }

    /// Marks both fields due and drops the running load, whose answer may
    /// predate changes made elsewhere. Shown values stay until a reload lands.
    func invalidate() {
        epoch += 1
        pending = nil
        threadEdits.removeAll()
        likesDue = true
        commentsDue = true
    }

    /// The store answers a toggle with the whole like set, so it counts as
    /// a fresh load of likes.
    func toggleLike(postId: String) -> Task<Void, Never> {
        Task {
            guard let next = try? await library.toggleLike(id: id, postId: postId) else { return }
            likeEdits += 1
            likes = next
            likesDue = false
        }
    }

    /// Applies an edited thread at once and keeps the other posts' counts.
    func threadChanged(postId: String, comments thread: [PostComment]) {
        comments = (comments ?? CommentIndex()).afterThread(postId: postId, comments: thread)
        if pending != nil { threadEdits[postId] = thread }
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
                            let social = model.social(for: hit.book.id)
                            TimelinePost(
                                post: hit.post,
                                author: hit.book.author,
                                handle: hit.book.handle,
                                cover: app.library.coverFile(id: hit.book.id),
                                liked: social?.likes?.contains(hit.post.id) == true,
                                commentCount: social?.comments?.count(hit.post.id) ?? 0,
                                commented: social?.comments?.commented(hit.post.id) == true,
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
