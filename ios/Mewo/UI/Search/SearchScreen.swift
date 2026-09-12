import SwiftUI

@Observable
@MainActor
final class SearchViewModel {
    var query = ""
    var result = SearchResult()
    var likes: [String: Set<String>] = [:]
    var comments: [String: CommentIndex] = [:]
    private let env: AppEnvironment

    init(env: AppEnvironment) { self.env = env }

    func setQuery(_ query: String) {
        self.query = query
        Task {
            do {
                let hits = try await env.library.search(query: query)
                let bookIds = Array(Set(hits.posts.map(\.book.id)))
                var likes: [String: Set<String>] = [:]
                var comments: [String: CommentIndex] = [:]
                for id in bookIds {
                    likes[id] = try await env.library.likes(id: id)
                    comments[id] = (try? await env.library.commentIndex(id: id)) ?? CommentIndex()
                }
                if self.query == query {
                    self.result = hits
                    self.likes.merge(likes) { _, new in new }
                    self.comments.merge(comments) { _, new in new }
                }
            } catch {}
        }
    }

    func toggleLike(bookId: String, postId: String) {
        Task {
            if let next = try? await env.library.toggleLike(id: bookId, postId: postId) {
                likes[bookId] = next
            }
        }
    }

    func onCommentsChanged(bookId: String, postId: String, comments: [PostComment]) {
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
        _model = State(initialValue: SearchViewModel(env: env))
    }

    var body: some View {
        VStack(spacing: 0) {
            MewoAppBar(leading: { ProfileButton(onClick: onOpenAccount) }) {
                TextField("Search books and posts", text: Binding(
                    get: { model.query },
                    set: { model.setQuery($0) }
                ))
                .font(MewoFont.regular(17))
                .foregroundStyle(mewo.ink)
                .focused($focused)
                .padding(.trailing, 16)
                .accessibilityIdentifier("search-field")
            }
            results
        }
        .background(mewo.ground)
        .onAppear { focused = true }
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
            hint("Type a title, author, or a line you remember.")
        } else if model.result.books.isEmpty && model.result.posts.isEmpty {
            hint("No matches for \"\(model.query)\".")
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

    private func hint(_ text: String) -> some View {
        Text(text)
            .font(MewoFont.regular(17))
            .foregroundStyle(mewo.mute)
            .padding(.horizontal, 16)
            .padding(.vertical, 24)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}
