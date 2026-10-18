import SwiftUI

private let searchTabKey = "search_tab"

/// Search state. Local scans publish through `result`. Hosted search waits
/// 250ms, and a page is applied only for the session token and query that
/// started it.
@Observable
@MainActor
final class SearchViewModel {
    var query = ""
    var tab: SearchTab
    var result = SearchResult()
    var searching = false
    var loadingMore = false
    var failure: String?
    private var slices: [SearchTab: SearchSlice] = [:]
    private var socials: [String: BookSocial] = [:]
    private let library: LibraryStore
    private var remote: RemoteSearch?
    private let defaults: UserDefaults?
    private let debounceNanos: UInt64
    private var sessionSeen: String?
    /// The running scan. Tests await it.
    @ObservationIgnored private(set) var searchTask: Task<Void, Never>?
    /// Bumped per scan, so a scan started for an older query (even the same
    /// text typed again) cannot publish.
    @ObservationIgnored private var generation = 0

    var hosted: Bool { remote != nil }

    init(
        library: LibraryStore,
        remote: RemoteSearch? = nil,
        defaults: UserDefaults? = nil,
        debounceNanos: UInt64 = searchDebounceNanos
    ) {
        self.library = library
        self.remote = remote
        self.defaults = defaults
        self.debounceNanos = debounceNanos
        self.sessionSeen = remote?.token
        let saved = defaults?.string(forKey: searchTabKey).flatMap(SearchTab.init(rawValue:))
        if remote != nil {
            tab = saved ?? .accounts
        } else if let saved, saved != .accounts {
            tab = saved
        } else {
            tab = .books
        }
    }

    func visibleAccounts() -> [AccountRecord] {
        slices[.accounts]?.accounts ?? []
    }

    func visibleBooks() -> [BookRecord] {
        hosted ? (slices[.books]?.books ?? []) : (tab == .books ? result.books : [])
    }

    func visiblePosts() -> [PostHit] {
        hosted ? (slices[.posts]?.posts ?? []) : (tab == .posts ? result.posts : [])
    }

    func showNoMatches() -> Bool {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty || searching || failure != nil { return false }
        if hosted {
            guard slices[tab]?.answeredQuery == trimmed else { return false }
        }
        switch tab {
        case .accounts: return visibleAccounts().isEmpty
        case .books: return visibleBooks().isEmpty
        case .posts: return visiblePosts().isEmpty
        }
    }

    var nextCursor: String? { slices[tab]?.nextCursor }

    /// Likes and comment counts for a book, once a search has hit it.
    func social(for bookId: String) -> BookSocial? { socials[bookId] }

    func configure(remote: RemoteSearch?) {
        let nowHosted = remote != nil
        if nowHosted == (self.remote != nil) {
            self.remote = remote
            sessionSeen = remote?.token ?? sessionSeen
            return
        }
        self.remote = remote
        sessionSeen = remote?.token
        if remote == nil, tab == .accounts { tab = .books }
        searchTask?.cancel()
        generation += 1
        slices = [:]
        searching = false
        loadingMore = false
        failure = nil
        if remote == nil {
            run()
        } else {
            schedule(append: false)
        }
    }

    /// Drops in-flight pages when the account changes. A response that was
    /// already on the way is ignored because it carries the old token.
    func noteSession(_ token: String?) {
        guard remote != nil else { return }
        if token == sessionSeen { return }
        sessionSeen = token
        searchTask?.cancel()
        generation += 1
        slices = [:]
        searching = false
        loadingMore = false
        failure = nil
        if token == nil || query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return }
        schedule(append: false)
    }

    func setQuery(_ query: String) {
        if query == self.query { return }
        self.query = query
        if remote == nil {
            run()
        } else {
            schedule(append: false)
        }
    }

    func select(_ tab: SearchTab) {
        if remote == nil && tab == .accounts { return }
        guard tab != self.tab else { return }
        self.tab = tab
        defaults?.set(tab.rawValue, forKey: searchTabKey)
        if remote != nil { schedule(append: false) }
    }

    func loadMore() {
        guard remote != nil, !searching, !loadingMore else { return }
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard slices[tab]?.answeredQuery == trimmed else { return }
        guard let cursor = slices[tab]?.nextCursor, !cursor.isEmpty else { return }
        schedule(append: true)
    }

    func retry() {
        guard remote != nil else { return }
        failure = nil
        schedule(append: false)
    }

    /// Searches again and marks every book's likes and comments due, since
    /// they may have changed in the reader. Shown values stay until the
    /// reload lands. Called when the screen appears.
    func refresh() {
        for social in socials.values { social.invalidate() }
        if remote == nil {
            run()
        } else {
            schedule(append: false)
        }
    }

    private func run() {
        searchTask?.cancel()
        generation += 1
        let token = generation
        let query = self.query
        if query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            searchTask = nil
            result = SearchResult()
            searching = false
            return
        }
        searching = true
        searchTask = Task {
            let hits = try? await library.search(query: query)
            guard token == generation else { return }
            if let hits {
                result = hits
                for hit in hits.posts { social(hit.book.id).load() }
            }
            searching = false
        }
    }

    private func schedule(append: Bool) {
        searchTask?.cancel()
        generation += 1
        let token = generation
        let query = self.query
        let tab = self.tab
        let cursor = append ? slices[tab]?.nextCursor : nil
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if !append && trimmed.isEmpty {
            searchTask = nil
            slices = [:]
            searching = false
            loadingMore = false
            failure = nil
            return
        }
        let session = remote?.token
        if append {
            loadingMore = true
        } else {
            searching = true
            failure = nil
        }
        searchTask = Task {
            if !append {
                do {
                    try await Task.sleep(nanoseconds: debounceNanos)
                } catch {
                    return
                }
            }
            guard !Task.isCancelled, token == generation else { return }
            guard let remote else { return }
            guard remote.token == session else {
                if token == generation {
                    searching = false
                    loadingMore = false
                }
                return
            }
            do {
                let page = try await remote.search(query: trimmed, type: tab, cursor: cursor)
                guard token == generation, remote.token == session, self.query == query, self.tab == tab else {
                    if token == generation {
                        searching = false
                        loadingMore = false
                    }
                    return
                }
                apply(page, append: append, query: trimmed, tab: tab)
            } catch is CancellationError {
                return
            } catch {
                guard token == generation, remote.token == session else { return }
                searching = false
                loadingMore = false
                self.failure = error.localizedDescription
            }
        }
    }

    private func apply(_ page: SearchPage, append: Bool, query: String, tab: SearchTab) {
        var slice = append ? (slices[tab] ?? SearchSlice()) : SearchSlice()
        switch tab {
        case .accounts:
            slice.accounts = append ? slice.accounts + page.accounts : page.accounts
        case .books:
            slice.books = append ? slice.books + page.books : page.books
        case .posts:
            let rows = page.posts
            slice.posts = append ? slice.posts + rows : rows
            for hit in rows { social(hit.book.id).load() }
        }
        slice.nextCursor = page.nextCursor
        slice.answeredQuery = query
        slices[tab] = slice
        searching = false
        loadingMore = false
        failure = nil
    }

    /// Returns the request so tests can await it.
    @discardableResult
    func toggleLike(bookId: String, postId: String) -> Task<Void, Never> {
        social(bookId).toggleLike(postId: postId)
    }

    @discardableResult
    func toggleRepost(bookId: String, postId: String) -> Task<Void, Never> {
        social(bookId).toggleRepost(postId: postId)
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

private struct SearchSlice {
    var accounts: [AccountRecord] = []
    var books: [BookRecord] = []
    var posts: [PostHit] = []
    var nextCursor: String?
    var answeredQuery: String?
}

@MainActor
protocol RemoteSearch: AnyObject {
    var token: String? { get }
    func search(query: String, type: SearchTab, cursor: String?) async throws -> SearchPage
}

@MainActor
final class HostedRemoteSearch: RemoteSearch {
    private let sessions: HostedSessionStore
    private let client: HostedClient
    var token: String? { sessions.session.token }

    init(sessions: HostedSessionStore, client: HostedClient) {
        self.sessions = sessions
        self.client = client
    }

    func search(query: String, type: SearchTab, cursor: String?) async throws -> SearchPage {
        let session = sessions.session
        return try await client.search(session, query: query, type: type.rawValue, cursor: cursor)
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
    /// Reposts still waiting on the server. Replayed onto a loaded index.
    @ObservationIgnored private let reposts = RepostOverlay()
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
                index = reposts.replay(over: index)
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

    func toggleRepost(postId: String) -> Task<Void, Never> {
        commitRepostToggle(
            bookId: id,
            postId: postId,
            read: { [weak self] in self?.comments ?? CommentIndex() },
            library: library,
            publish: { [weak self] next in self?.comments = next },
            overlay: reposts
        )
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
        let remote: RemoteSearch? = env.backend.kind == .hosted
            ? HostedRemoteSearch(sessions: env.hostedSession, client: env.client)
            : nil
        _model = State(initialValue: SearchViewModel(
            library: env.library,
            remote: remote,
            defaults: UserDefaults(suiteName: "mewo")
        ))
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
            UnderlineTabs(
                items: tabItems,
                selected: model.tab.rawValue,
                idPrefix: "search-tab",
                onSelect: { id in
                    if let tab = SearchTab(rawValue: id) { model.select(tab) }
                }
            )
            results
        }
        .background(mewo.ground)
        .onAppear {
            focused = true
            model.configure(remote: hostedRemote)
            model.noteSession(app.hostedSession.session.token)
            model.refresh()
        }
        .onChange(of: app.backend.kind) { _, _ in
            model.configure(remote: hostedRemote)
        }
        .onChange(of: app.hostedSession.session.token) { _, token in
            model.noteSession(token)
        }
        .onChange(of: model.query) { _, q in
            chrome.showWhenIdle(q.isEmpty || model.showNoMatches())
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

    private var hostedRemote: RemoteSearch? {
        app.backend.kind == .hosted
            ? HostedRemoteSearch(sessions: app.hostedSession, client: app.client)
            : nil
    }

    private var tabItems: [UnderlineTabItem] {
        let tabs: [SearchTab] = model.hosted ? SearchTab.allCases : [.books, .posts]
        return tabs.map { UnderlineTabItem(id: $0.rawValue, title: $0.title) }
    }

    @ViewBuilder
    private var results: some View {
        if model.query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            Color.clear
        } else if model.showNoMatches() {
            Text(noMatchesCopy)
                .font(MewoFont.bold(22))
                .foregroundStyle(mewo.ink)
                .padding(.horizontal, 28)
                .padding(.vertical, 48)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        } else if model.searching && currentRowsEmpty && model.failure == nil {
            ProgressView()
                .tint(mewo.accent)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                .padding(.top, 24)
                .accessibilityIdentifier("search-loading")
        } else if currentRowsEmpty, let failure = model.failure {
            failureRow(failure)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    GeometryReader { geo in
                        Color.clear.preference(key: ScrollOffsetKey.self, value: -geo.frame(in: .named("search")).minY)
                    }
                    .frame(height: 0)
                    if model.searching {
                        ProgressView()
                            .tint(mewo.accent)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 12)
                            .accessibilityIdentifier("search-loading")
                    }
                    if let failure = model.failure {
                        failureRow(failure)
                    }
                    rows
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

    private var currentRowsEmpty: Bool {
        switch model.tab {
        case .accounts: return model.visibleAccounts().isEmpty
        case .books: return model.visibleBooks().isEmpty
        case .posts: return model.visiblePosts().isEmpty
        }
    }

    private func failureRow(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(message)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.mute)
            Button("Try again") { model.retry() }
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.accent)
                .buttonStyle(.plain)
                .accessibilityIdentifier("search-retry")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    @ViewBuilder
    private var rows: some View {
        switch model.tab {
        case .accounts:
            ForEach(model.visibleAccounts()) { account in
                Button {
                    onOpenProfile(account.username, account.username)
                } label: {
                    HStack(spacing: 12) {
                        Avatar(name: account.username, size: 40)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(account.username).font(MewoFont.bold(17)).foregroundStyle(mewo.ink)
                            Text("@\(account.username)").font(MewoFont.regular(13)).foregroundStyle(mewo.mute)
                        }
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("account-\(account.username)")
                .onAppear { if account.id == model.visibleAccounts().last?.id { model.loadMore() } }
                Hairline()
            }
        case .books:
            ForEach(model.visibleBooks()) { book in
                Button { onOpenBook(book.id) } label: {
                    HStack(spacing: 12) {
                        Avatar(name: book.author, cover: app.library.coverFile(id: book.id), size: 40)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(book.title).font(MewoFont.bold(15)).foregroundStyle(mewo.ink)
                            Text("@\(book.handle)").font(MewoFont.regular(13)).foregroundStyle(mewo.mute)
                        }
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .onAppear { if book.id == model.visibleBooks().last?.id { model.loadMore() } }
                Hairline()
            }
        case .posts:
            ForEach(model.visiblePosts()) { hit in
                let social = model.social(for: hit.book.id)
                TimelinePost(
                    post: hit.post,
                    author: hit.book.author,
                    handle: hit.book.handle,
                    cover: app.library.coverFile(id: hit.book.id),
                    liked: social?.likes?.contains(hit.post.id) == true,
                    commentCount: social?.comments?.count(hit.post.id) ?? 0,
                    commented: social?.comments?.commented(hit.post.id) == true,
                    reposted: social?.comments?.didRepost(hit.post.id) == true,
                    repostCount: social?.comments?.repostCount(hit.post.id) ?? 0,
                    onComment: { commentTarget = CommentTarget(book: hit.book, post: hit.post) },
                    onLike: { model.toggleLike(bookId: hit.book.id, postId: hit.post.id) },
                    onRepost: { model.toggleRepost(bookId: hit.book.id, postId: hit.post.id) },
                    onQuote: { shareText = quoteLine(author: hit.book.author, title: hit.book.title, text: hit.post.text) },
                    onShare: { shareText = hit.post.text },
                    onOpen: { onOpenPost(hit.book.id, hit.post.id) }
                )
                .onAppear { if hit.id == model.visiblePosts().last?.id { model.loadMore() } }
            }
        }
    }
}
