import SwiftUI

enum ProfileTab: String, CaseIterable, Identifiable {
    case posts, reposts, books

    var id: String { rawValue }

    var title: String {
        switch self {
        case .posts: return "Posts"
        case .reposts: return "Reposts"
        case .books: return "Books"
        }
    }

    var emptyTitle: String {
        switch self {
        case .posts: return "No posts"
        case .reposts: return "No posts"
        case .books: return "No books"
        }
    }
}

@MainActor
protocol ProfileLoading: AnyObject {
    var token: String? { get }
    func loadProfile(username: String) async throws -> ProfileRecord
    func loadPosts(username: String, cursor: String?) async throws -> ProfilePostsPage
    func loadReposts(username: String, cursor: String?) async throws -> ProfileRepostsPage
    func loadBooks(username: String) async throws -> ProfileBooksPage
}

@MainActor
final class HostedProfileLoader: ProfileLoading {
    private let sessions: HostedSessionStore
    private let client: HostedClient
    var token: String? { sessions.session.token }

    init(sessions: HostedSessionStore, client: HostedClient) {
        self.sessions = sessions
        self.client = client
    }

    func loadProfile(username: String) async throws -> ProfileRecord {
        try await client.profile(sessions.session, username: username)
    }

    func loadPosts(username: String, cursor: String?) async throws -> ProfilePostsPage {
        try await client.profilePosts(sessions.session, username: username, cursor: cursor)
    }

    func loadReposts(username: String, cursor: String?) async throws -> ProfileRepostsPage {
        try await client.profileReposts(sessions.session, username: username, cursor: cursor)
    }

    func loadBooks(username: String) async throws -> ProfileBooksPage {
        try await client.profileBooks(sessions.session, username: username)
    }
}

@Observable
@MainActor
final class ProfileViewModel {
    var handle: String
    var tab: ProfileTab = .posts
    var record: ProfileRecord?
    var missing = false
    var headerLoading = false
    var headerFailure: String?
    var posts: [ProfileReply] = []
    var reposts: [ProfileRepostItem] = []
    var books: [BookRecord] = []
    var postsCursor: String?
    var repostsCursor: String?
    var postsLoading = false
    var repostsLoading = false
    var booksLoading = false
    var postsLoaded = false
    var repostsLoaded = false
    var booksLoaded = false
    var postsFailure: String?
    var repostsFailure: String?
    var booksFailure: String?
    var likes: [String: Set<String>] = [:]
    var comments: [String: CommentIndex] = [:]
    private let library: LibraryStore
    private var loader: ProfileLoading?
    private var sessionSeen: String?
    private var generation = 0
    private let hosted: Bool

    var postCount: Int? {
        if let record { return record.postCount }
        if !hosted && postsLoaded { return posts.count }
        return nil
    }

    init(env: AppEnvironment, handle: String) {
        self.library = env.library
        self.handle = handle
        self.hosted = env.backend.kind == .hosted
        if hosted {
            loader = HostedProfileLoader(sessions: env.hostedSession, client: env.client)
        }
        sessionSeen = loader?.token
    }

    init(loader: ProfileLoading?, library: LibraryStore, handle: String) {
        self.loader = loader
        self.library = library
        self.handle = handle
        self.hosted = loader != nil
        self.sessionSeen = loader?.token
    }

    func noteSession(_ token: String?) {
        guard hosted else { return }
        if token == sessionSeen { return }
        sessionSeen = token
        generation += 1
        record = nil
        missing = false
        headerLoading = false
        posts = []
        reposts = []
        books = []
        postsLoaded = false
        repostsLoaded = false
        booksLoaded = false
        postsLoading = false
        repostsLoading = false
        booksLoading = false
        headerFailure = nil
        if token != nil { refresh() }
    }

    func refresh() {
        generation += 1
        let token = generation
        let session = loader?.token
        if hosted {
            headerLoading = record == nil && !missing
            headerFailure = nil
            Task { await loadHeader(token: token, session: session) }
        } else {
            headerLoading = false
            Task { await loadLocalPosts(token: token) }
        }
    }

    func select(_ tab: ProfileTab) {
        guard tab != self.tab else { return }
        self.tab = tab
        let token = generation
        let session = loader?.token
        Task { await loadTab(tab, token: token, session: session, append: false) }
    }

    func loadMore() {
        let token = generation
        let session = loader?.token
        Task { await loadTab(tab, token: token, session: session, append: true) }
    }

    func retryTab() {
        let token = generation
        let session = loader?.token
        Task { await loadTab(tab, token: token, session: session, append: false) }
    }

    func toggleLike(bookId: String, postId: String) {
        Task {
            if let ids = try? await library.toggleLike(id: bookId, postId: postId) {
                likes[bookId] = ids
            }
        }
    }

    func toggleRepost(bookId: String, postId: String) {
        commitRepostToggle(
            bookId: bookId,
            postId: postId,
            read: { self.comments[bookId] ?? CommentIndex() },
            library: library,
            publish: { self.comments[bookId] = $0 }
        )
    }

    func onCommentsChanged(bookId: String, postId: String, comments: [PostComment]) {
        let current = self.comments[bookId] ?? CommentIndex()
        posts.removeAll { hit in
            hit.book.id == bookId && hit.post.id == postId && !comments.contains { $0.id == hit.comment.id }
        }
        self.comments[bookId] = current.afterThread(postId: postId, comments: comments)
    }

    private func loadHeader(token: Int, session: String?) async {
        guard let loader else { return }
        do {
            let profile = try await loader.loadProfile(username: handle)
            guard still(token, session) else { return }
            record = profile
            missing = false
            headerLoading = false
            headerFailure = nil
            await loadTab(.posts, token: token, session: session, append: false)
        } catch let err as HostedException where err.code == 404 {
            guard still(token, session) else { return }
            missing = true
            headerLoading = false
            headerFailure = nil
        } catch is CancellationError {
            return
        } catch {
            guard still(token, session) else { return }
            headerLoading = false
            if record == nil { headerFailure = error.localizedDescription }
        }
    }

    private func still(_ token: Int, _ session: String?) -> Bool {
        guard token == generation, loader?.token == session else {
            if token == generation {
                headerLoading = false
                postsLoading = false
                repostsLoading = false
                booksLoading = false
            }
            return false
        }
        return true
    }

    private func loadLocalPosts(token: Int) async {
        postsLoading = posts.isEmpty
        do {
            let replies = try await library.profileReplies(handle: handle)
            guard token == generation else { return }
            posts = replies
            postsLoaded = true
            postsLoading = false
            postsFailure = nil
            repostsLoaded = true
            booksLoaded = true
            await loadSocial(for: Set(replies.map(\.book.id)))
        } catch {
            guard token == generation else { return }
            postsLoading = false
            postsFailure = error.localizedDescription
        }
    }

    private func loadTab(_ tab: ProfileTab, token: Int, session: String?, append: Bool) async {
        guard hosted, let loader else { return }
        guard still(token, session) else { return }
        switch tab {
        case .posts:
            if append {
                guard let cursor = postsCursor, !cursor.isEmpty, !postsLoading else { return }
            } else if postsLoading { return }
            postsLoading = true
            postsFailure = nil
            do {
                let page = try await loader.loadPosts(username: handle, cursor: append ? postsCursor : nil)
                guard still(token, session) else { return }
                let rows = page.items.map { ProfileReply(book: $0.book, post: $0.post, comment: $0.comment) }
                posts = append ? posts + rows : rows
                postsCursor = page.nextCursor
                postsLoaded = true
                postsLoading = false
                await loadSocial(for: Set(rows.map(\.book.id)))
            } catch is CancellationError {
                return
            } catch {
                guard still(token, session) else { return }
                postsLoading = false
                postsFailure = error.localizedDescription
            }
        case .reposts:
            if append {
                guard let cursor = repostsCursor, !cursor.isEmpty, !repostsLoading else { return }
            } else if repostsLoading { return }
            repostsLoading = true
            repostsFailure = nil
            do {
                let page = try await loader.loadReposts(username: handle, cursor: append ? repostsCursor : nil)
                guard still(token, session) else { return }
                reposts = append ? reposts + page.items : page.items
                repostsCursor = page.nextCursor
                repostsLoaded = true
                repostsLoading = false
                await loadSocial(for: Set(page.items.map(\.book.id)))
            } catch is CancellationError {
                return
            } catch {
                guard still(token, session) else { return }
                repostsLoading = false
                repostsFailure = error.localizedDescription
            }
        case .books:
            if booksLoading { return }
            booksLoading = true
            booksFailure = nil
            do {
                let page = try await loader.loadBooks(username: handle)
                guard still(token, session) else { return }
                books = page.items
                booksLoaded = true
                booksLoading = false
            } catch is CancellationError {
                return
            } catch {
                guard still(token, session) else { return }
                booksLoading = false
                booksFailure = error.localizedDescription
            }
        }
    }

    private func loadSocial(for ids: Set<String>) async {
        for id in ids {
            likes[id] = (try? await library.likes(id: id)) ?? likes[id] ?? []
            comments[id] = (try? await library.commentIndex(id: id)) ?? comments[id] ?? CommentIndex()
        }
    }
}

struct ProfileScreen: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Bindable var chrome: HideOnScrollState
    var displayName: String
    var onBack: () -> Void
    var onOpenBook: (String) -> Void
    var onOpenPost: (String, String) -> Void
    var onOpenProfile: (String, String) -> Void
    @State private var model: ProfileViewModel
    @State private var commentTarget: CommentTarget?
    @State private var shareText: String?
    @State private var lastOffset: CGFloat = 0

    init(
        env: AppEnvironment,
        handle: String,
        displayName: String,
        chrome: HideOnScrollState,
        onBack: @escaping () -> Void,
        onOpenBook: @escaping (String) -> Void,
        onOpenPost: @escaping (String, String) -> Void,
        onOpenProfile: @escaping (String, String) -> Void
    ) {
        self.displayName = displayName
        self.chrome = chrome
        self.onBack = onBack
        self.onOpenBook = onOpenBook
        self.onOpenPost = onOpenPost
        self.onOpenProfile = onOpenProfile
        _model = State(initialValue: ProfileViewModel(env: env, handle: handle))
    }

    var body: some View {
        let me = app.readerIdentity
        let viewingSelf = model.handle.caseInsensitiveCompare(me.handle) == .orderedSame
        let name = model.record?.username ?? (viewingSelf ? me.name : displayName.ifBlank { model.handle })
        let handle = model.record?.username ?? (viewingSelf ? me.handle : model.handle)
        VStack(spacing: 0) {
            MewoAppBar(visible: chrome.visible, leading: {
                Button(action: onBack) {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 18, weight: .semibold))
                        .foregroundStyle(mewo.ink)
                        .frame(width: 44, height: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Back")
            }, content: {
                VStack(alignment: .leading, spacing: 0) {
                    Text(name).font(MewoFont.bold(15)).foregroundStyle(mewo.ink).lineLimit(1)
                    if let count = model.postCount {
                        Text(postCountLabel(count))
                            .font(MewoFont.regular(13))
                            .foregroundStyle(mewo.mute)
                    }
                }
                .padding(.trailing, 16)
            })
            if model.missing {
                Text(missingAccountCopy)
                    .font(MewoFont.bold(22))
                    .foregroundStyle(mewo.ink)
                    .padding(.horizontal, 28)
                    .padding(.vertical, 48)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            } else if model.headerLoading && model.record == nil {
                ProgressView().tint(mewo.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error = model.headerFailure, model.record == nil {
                VStack(alignment: .leading, spacing: 8) {
                    Text(error).font(MewoFont.regular(15)).foregroundStyle(mewo.mute)
                    Button("Try again") { model.refresh() }
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.accent)
                        .buttonStyle(.plain)
                }
                .padding(16)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        GeometryReader { geo in
                            Color.clear.preference(key: ScrollOffsetKey.self, value: -geo.frame(in: .named("profile")).minY)
                        }
                        .frame(height: 0)
                        header(name: name, handle: handle)
                        tabBody
                    }
                }
                .coordinateSpace(name: "profile")
                .onPreferenceChange(ScrollOffsetKey.self) { offset in
                    chrome.ingest(delta: offset - lastOffset)
                    lastOffset = offset
                    chrome.bindTop(offset)
                }
            }
        }
        .background(mewo.ground)
        .onAppear {
            model.noteSession(app.hostedSession.session.token)
            model.refresh()
            chrome.showWhenIdle(model.headerLoading || model.posts.isEmpty)
        }
        .onChange(of: app.hostedSession.session.token) { _, token in
            model.noteSession(token)
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

    private func header(name: String, handle: String) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            ZStack(alignment: .topLeading) {
                VStack(alignment: .leading, spacing: 0) {
                    mewo.hover.frame(height: 125)
                    Spacer().frame(height: 42)
                    Text(name).font(MewoFont.bold(20)).foregroundStyle(mewo.ink).padding(.horizontal, 16)
                    Text("@\(handle)").font(MewoFont.regular(13)).foregroundStyle(mewo.mute).padding(.horizontal, 16).padding(.top, 2)
                    if let joined = model.record.map({ joinedLabel($0.joinedAt) }) {
                        Text(joined)
                            .font(MewoFont.regular(13))
                            .foregroundStyle(mewo.mute)
                            .padding(.horizontal, 16)
                            .padding(.top, 2)
                    }
                    Spacer().frame(height: 16)
                }
                Circle()
                    .fill(mewo.ground)
                    .frame(width: 76, height: 76)
                    .overlay(Avatar(name: name, size: 68))
                    .padding(.leading, 12)
                    .padding(.top, 87)
            }
            UnderlineTabs(
                items: ProfileTab.allCases.map { UnderlineTabItem(id: $0.rawValue, title: $0.title) },
                selected: model.tab.rawValue,
                idPrefix: "profile-tab",
                onSelect: { id in
                    if let tab = ProfileTab(rawValue: id) { model.select(tab) }
                }
            )
        }
    }

    @ViewBuilder
    private var tabBody: some View {
        switch model.tab {
        case .posts:
            tabList(
                empty: model.posts.isEmpty,
                loading: model.postsLoading,
                loaded: model.postsLoaded,
                failure: model.postsFailure,
                title: ProfileTab.posts.emptyTitle
            ) {
                ForEach(model.posts, id: \.comment.id) { hit in
                    replyRow(hit)
                        .onAppear {
                            if hit.comment.id == model.posts.last?.comment.id { model.loadMore() }
                        }
                }
            }
        case .reposts:
            tabList(
                empty: model.reposts.isEmpty,
                loading: model.repostsLoading,
                loaded: model.repostsLoaded,
                failure: model.repostsFailure,
                title: ProfileTab.reposts.emptyTitle
            ) {
                ForEach(Array(model.reposts.enumerated()), id: \.offset) { index, hit in
                    repostRow(hit)
                        .onAppear {
                            if index == model.reposts.count - 1 { model.loadMore() }
                        }
                }
            }
        case .books:
            tabList(
                empty: model.books.isEmpty,
                loading: model.booksLoading,
                loaded: model.booksLoaded,
                failure: model.booksFailure,
                title: ProfileTab.books.emptyTitle
            ) {
                ForEach(model.books) { book in
                    Button { onOpenBook(book.id) } label: {
                        bookRow(book)
                    }
                    .buttonStyle(.plain)
                    Hairline()
                }
            }
        }
    }

    @ViewBuilder
    private func tabList<Rows: View>(
        empty: Bool,
        loading: Bool,
        loaded: Bool,
        failure: String?,
        title: String,
        @ViewBuilder rows: () -> Rows
    ) -> some View {
        if loading && empty {
            ProgressView().tint(mewo.accent).frame(maxWidth: .infinity).padding(.vertical, 32)
        } else if let failure, empty {
            VStack(alignment: .leading, spacing: 8) {
                Text(failure).font(MewoFont.regular(15)).foregroundStyle(mewo.mute)
                Button("Try again") { model.retryTab() }
                    .font(MewoFont.regular(15))
                    .foregroundStyle(mewo.accent)
                    .buttonStyle(.plain)
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
        } else if loaded && empty {
            Text(title)
                .font(MewoFont.bold(22))
                .foregroundStyle(mewo.ink)
                .padding(.horizontal, 28)
                .padding(.vertical, 48)
                .frame(maxWidth: .infinity, alignment: .leading)
        } else {
            if let failure {
                VStack(alignment: .leading, spacing: 8) {
                    Text(failure).font(MewoFont.regular(15)).foregroundStyle(mewo.mute)
                    Button("Try again") { model.retryTab() }
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.accent)
                        .buttonStyle(.plain)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            rows()
            if loading {
                ProgressView().tint(mewo.accent).frame(maxWidth: .infinity).padding(.vertical, 12)
            }
        }
    }

    private func replyRow(_ hit: ProfileReply) -> some View {
        let identity = app.commenterIdentity(hit.comment)
        let index = model.comments[hit.book.id]
        return PassageReply(
            passageName: hit.book.author,
            passageHandle: hit.book.handle,
            passageMeta: hit.post.chapter,
            passageText: hit.post.text,
            passageHeading: hit.post.kind == .heading,
            passageCover: app.library.coverFile(id: hit.book.id),
            actorName: identity.name,
            actorHandle: identity.handle,
            actorMeta: relativeTime(createdAt: hit.comment.createdAt),
            actorText: hit.comment.text,
            liked: model.likes[hit.book.id]?.contains(hit.post.id) == true,
            commentCount: index?.count(hit.post.id) ?? 0,
            commented: index?.commented(hit.post.id) == true,
            reposted: index?.didRepost(hit.post.id) == true,
            repostCount: index?.repostCount(hit.post.id) ?? 0,
            onOpenPassage: { onOpenPost(hit.book.id, hit.post.id) },
            onOpenActor: { onOpenProfile(identity.handle, identity.name) },
            onComment: { commentTarget = CommentTarget(book: hit.book, post: hit.post) },
            onLike: { model.toggleLike(bookId: hit.book.id, postId: hit.post.id) },
            onRepost: { model.toggleRepost(bookId: hit.book.id, postId: hit.post.id) },
            onQuote: { shareText = quoteLine(author: hit.book.author, title: hit.book.title, text: hit.post.text) },
            onShare: { shareText = hit.post.text }
        )
    }

    private func repostRow(_ hit: ProfileRepostItem) -> some View {
        let index = model.comments[hit.book.id]
        return VStack(spacing: 0) {
            TimelinePost(
                post: hit.post,
                author: hit.book.author,
                handle: hit.book.handle,
                cover: app.library.coverFile(id: hit.book.id),
                liked: model.likes[hit.book.id]?.contains(hit.post.id) == true,
                commentCount: index?.count(hit.post.id) ?? 0,
                commented: index?.commented(hit.post.id) == true,
                reposted: index?.didRepost(hit.post.id) == true,
                repostCount: index?.repostCount(hit.post.id) ?? 0,
                onComment: { commentTarget = CommentTarget(book: hit.book, post: hit.post) },
                onLike: { model.toggleLike(bookId: hit.book.id, postId: hit.post.id) },
                onRepost: { model.toggleRepost(bookId: hit.book.id, postId: hit.post.id) },
                onQuote: { shareText = quoteLine(author: hit.book.author, title: hit.book.title, text: hit.post.text) },
                onShare: { shareText = hit.post.text },
                onOpen: { onOpenPost(hit.book.id, hit.post.id) }
            )
        }
    }

    private func bookRow(_ book: BookRecord) -> some View {
        HStack(spacing: 12) {
            Avatar(name: book.author, cover: app.library.coverFile(id: book.id), size: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(book.title).font(MewoFont.bold(15)).foregroundStyle(mewo.ink)
                Text("@\(book.handle)").font(MewoFont.regular(13)).foregroundStyle(mewo.mute)
                Text(statusLine(for: book)).font(MewoFont.regular(15)).foregroundStyle(mewo.ink)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .contentShape(Rectangle())
    }
}
