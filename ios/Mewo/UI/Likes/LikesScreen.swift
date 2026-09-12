import SwiftUI

@Observable
@MainActor
final class LikesViewModel {
    var hits: [PostHit] = []
    var comments: [String: CommentIndex] = [:]
    var loading = true
    private let env: AppEnvironment

    init(env: AppEnvironment) { self.env = env }

    func refresh() {
        Task {
            loading = hits.isEmpty
            do {
                let hits = try await env.library.likedPosts()
                var comments: [String: CommentIndex] = [:]
                for id in Set(hits.map(\.book.id)) {
                    comments[id] = (try? await env.library.commentIndex(id: id)) ?? CommentIndex()
                }
                self.hits = hits
                self.comments = comments
                loading = false
            } catch {
                hits = []
                loading = false
            }
        }
    }

    func unlike(bookId: String, postId: String) {
        Task {
            _ = try? await env.library.toggleLike(id: bookId, postId: postId)
            hits.removeAll { $0.post.id == postId && $0.book.id == bookId }
        }
    }

    func onCommentsChanged(bookId: String, postId: String, comments: [PostComment]) {
        let current = self.comments[bookId] ?? CommentIndex()
        self.comments[bookId] = current.afterThread(postId: postId, comments: comments)
    }
}

struct LikesScreen: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Bindable var chrome: HideOnScrollState
    var onOpenPost: (String, String) -> Void
    var onOpenAccount: () -> Void
    var onOpenProfile: (String, String) -> Void
    @State private var model: LikesViewModel
    @State private var commentTarget: CommentTarget?
    @State private var shareText: String?
    @State private var lastOffset: CGFloat = 0

    init(env: AppEnvironment, chrome: HideOnScrollState, onOpenPost: @escaping (String, String) -> Void, onOpenAccount: @escaping () -> Void, onOpenProfile: @escaping (String, String) -> Void) {
        self.chrome = chrome
        self.onOpenPost = onOpenPost
        self.onOpenAccount = onOpenAccount
        self.onOpenProfile = onOpenProfile
        _model = State(initialValue: LikesViewModel(env: env))
    }

    var body: some View {
        VStack(spacing: 0) {
            MewoAppBar(visible: chrome.visible, leading: { ProfileButton(onClick: onOpenAccount) }) {
                Text("Likes").font(MewoFont.bold(20)).foregroundStyle(mewo.ink).padding(.trailing, 16)
            }
            if model.loading {
                ProgressView().tint(mewo.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if model.hits.isEmpty {
                Text("Heart a line while you read. It shows up here.")
                    .font(MewoFont.regular(17))
                    .foregroundStyle(mewo.mute)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 24)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        GeometryReader { geo in
                            Color.clear.preference(key: ScrollOffsetKey.self, value: -geo.frame(in: .named("likes")).minY)
                        }
                        .frame(height: 0)
                        ForEach(model.hits) { hit in
                            TimelinePost(
                                post: hit.post,
                                author: hit.book.author,
                                handle: hit.book.handle,
                                cover: app.library.coverFile(id: hit.book.id),
                                liked: true,
                                commentCount: model.comments[hit.book.id]?.count(hit.post.id) ?? 0,
                                commented: model.comments[hit.book.id]?.commented(hit.post.id) == true,
                                onComment: { commentTarget = CommentTarget(book: hit.book, post: hit.post) },
                                onLike: { model.unlike(bookId: hit.book.id, postId: hit.post.id) },
                                onRepost: { shareText = quoteLine(author: hit.book.author, title: hit.book.title, text: hit.post.text) },
                                onShare: { shareText = hit.post.text },
                                onOpen: { onOpenPost(hit.book.id, hit.post.id) }
                            )
                        }
                    }
                }
                .coordinateSpace(name: "likes")
                .onPreferenceChange(ScrollOffsetKey.self) { offset in
                    chrome.ingest(delta: offset - lastOffset)
                    lastOffset = offset
                    chrome.bindTop(offset)
                }
            }
        }
        .background(mewo.ground)
        .onAppear {
            model.refresh()
            chrome.showWhenIdle(model.loading || model.hits.isEmpty)
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
}
