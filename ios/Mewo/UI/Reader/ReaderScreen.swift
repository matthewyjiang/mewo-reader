import SwiftUI

@Observable
@MainActor
final class ReaderViewModel {
    var book: BookRecord?
    var posts: [FeedPost] = []
    var likes: Set<String> = []
    var comments = CommentIndex()
    var startIndex = 0
    var loading = true
    var error: String?
    private let env: AppEnvironment
    private let bookId: String
    private let focusPostId: String?

    init(env: AppEnvironment, bookId: String, focusPostId: String?) {
        self.env = env
        self.bookId = bookId
        self.focusPostId = focusPostId
    }

    func load() async {
        do {
            var found = env.library.book(id: bookId)
            if found == nil {
                try await env.library.load()
                found = env.library.book(id: bookId)
            }
            guard found != nil else { throw NSError(domain: "mewo", code: 11, userInfo: [NSLocalizedDescriptionKey: "That book is gone."]) }
            let posts = try await env.library.feed(id: bookId)
            let book = env.library.book(id: bookId) ?? found!
            let likes = try await env.library.likes(id: bookId)
            let comments = try await env.library.commentIndex(id: bookId)
            let focus = focusPostId.flatMap { id in posts.firstIndex(where: { $0.id == id }) }
            let start = focus ?? book.progressIndex.clamped(to: 0...max(0, posts.count - 1))
            self.book = book
            self.posts = posts
            self.likes = likes
            self.comments = comments
            self.startIndex = start
            self.loading = false
        } catch {
            loading = false
            self.error = error.localizedDescription
        }
    }

    func toggleLike(_ postId: String) {
        Task { likes = (try? await env.library.toggleLike(id: bookId, postId: postId)) ?? likes }
    }

    func onCommentsChanged(postId: String, comments: [PostComment]) {
        self.comments = self.comments.afterThread(postId: postId, comments: comments)
    }

    func saveProgress(_ index: Int) {
        Task { try? await env.library.saveProgress(id: bookId, index: index) }
    }
}

struct ReaderScreen: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Bindable var chrome: HideOnScrollState
    var onBack: () -> Void
    var onOpenProfile: (String, String) -> Void
    @State private var model: ReaderViewModel
    @State private var showChapters = false
    @State private var commentTarget: CommentTarget?
    @State private var shareText: String?
    @State private var lastOffset: CGFloat = 0
    @State private var listGeneration = 0
    @State private var jumpIndex: Int?

    init(env: AppEnvironment, bookId: String, postId: String?, chrome: HideOnScrollState, onBack: @escaping () -> Void, onOpenProfile: @escaping (String, String) -> Void) {
        self.chrome = chrome
        self.onBack = onBack
        self.onOpenProfile = onOpenProfile
        _model = State(initialValue: ReaderViewModel(env: env, bookId: bookId, focusPostId: postId))
    }

    var body: some View {
        VStack(spacing: 0) {
            MewoAppBar(
                visible: chrome.visible,
                leading: {
                    Button(action: onBack) {
                        Image(systemName: "chevron.left")
                            .font(.system(size: 18, weight: .semibold))
                            .foregroundStyle(mewo.ink)
                            .frame(width: 44, height: 44)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Back")
                },
                center: {
                    Button {
                        if !model.loading && model.error == nil { showChapters = true }
                    } label: {
                        Text(model.book?.title ?? "Post")
                            .font(MewoFont.bold(15))
                            .foregroundStyle(mewo.ink)
                            .lineLimit(1)
                            .padding(.horizontal, 56)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Chapters")
                }
            )
            content
        }
        .background(mewo.ground)
        .task { await model.load() }
        .onChange(of: model.loading) { _, loading in
            chrome.showWhenIdle(loading || model.error != nil)
        }
        .sheet(item: $commentTarget) { target in
            CommentSheet(
                target: target,
                publicNotes: app.backend.kind == .hosted,
                onThreadChanged: model.onCommentsChanged,
                onDismiss: { commentTarget = nil },
                onOpenProfile: { handle, name in
                    commentTarget = nil
                    onOpenProfile(handle, name)
                }
            )
        }
        .sheet(isPresented: $showChapters) {
            let headings = model.posts.headingPosts()
            ChapterSheet(
                title: model.book?.title ?? "Chapters",
                headings: headings,
                currentIndex: currentHeadingIndex(posts: model.posts, headings: headings, visibleIndex: jumpIndex ?? model.startIndex),
                onJump: { heading in
                    if let index = model.posts.firstIndex(where: { $0.id == heading.id }) {
                        jumpIndex = index
                        listGeneration += 1
                        model.saveProgress(index)
                    }
                    chrome.show()
                    showChapters = false
                },
                onDismiss: { showChapters = false }
            )
        }
        .sheet(isPresented: Binding(get: { shareText != nil }, set: { if !$0 { shareText = nil } })) {
            if let shareText {
                ShareSheet(text: shareText)
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        if model.loading {
            ProgressView().tint(mewo.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let error = model.error {
            VStack(spacing: 12) {
                Text(error).font(MewoFont.regular(17)).foregroundStyle(mewo.ink)
                Button("Back", action: onBack).foregroundStyle(mewo.accent)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            feed
        }
    }

    private var feed: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0) {
                    GeometryReader { geo in
                        Color.clear.preference(key: ScrollOffsetKey.self, value: -geo.frame(in: .named("reader")).minY)
                    }
                    .frame(height: 0)
                    ForEach(Array(model.posts.enumerated()), id: \.element.id) { index, post in
                        TimelinePost(
                            post: post,
                            author: model.book?.author ?? "Author",
                            handle: model.book?.handle ?? "author",
                            cover: model.book.flatMap { app.library.coverFile(id: $0.id) },
                            liked: model.likes.contains(post.id),
                            commentCount: model.comments.count(post.id),
                            commented: model.comments.commented(post.id),
                            onComment: { if let book = model.book { commentTarget = CommentTarget(book: book, post: post) } },
                            onLike: { model.toggleLike(post.id) },
                            onRepost: { shareText = quoteLine(author: model.book?.author, title: model.book?.title, text: post.text) },
                            onShare: { shareText = post.text },
                            onChapterClick: { showChapters = true }
                        )
                        .id(post.id)
                        .onAppear { model.saveProgress(index) }
                    }
                    Text(model.book.map { "End of \($0.title)" } ?? "You're caught up")
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.mute)
                        .frame(maxWidth: .infinity)
                        .padding(.horizontal, 24)
                        .padding(.vertical, 48)
                }
            }
            .coordinateSpace(name: "reader")
            .onPreferenceChange(ScrollOffsetKey.self) { offset in
                chrome.ingest(delta: offset - lastOffset)
                lastOffset = offset
                chrome.bindTop(offset)
            }
            .onAppear {
                let start = jumpIndex ?? model.startIndex
                if model.posts.indices.contains(start) {
                    proxy.scrollTo(model.posts[start].id, anchor: .top)
                }
            }
            .onChange(of: listGeneration) { _, _ in
                if let jumpIndex, model.posts.indices.contains(jumpIndex) {
                    proxy.scrollTo(model.posts[jumpIndex].id, anchor: .top)
                }
            }
        }
    }
}

extension Int {
    func clamped(to range: ClosedRange<Int>) -> Int {
        Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
    }
}
