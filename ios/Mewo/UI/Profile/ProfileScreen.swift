import SwiftUI

@Observable
@MainActor
final class ProfileViewModel {
    var handle: String
    var replies: [ProfileReply] = []
    var likes: [String: Set<String>] = [:]
    var comments: [String: CommentIndex] = [:]
    var loading = true
    var error: String?
    private let env: AppEnvironment

    init(env: AppEnvironment, handle: String) {
        self.env = env
        self.handle = handle
    }

    func refresh() {
        Task {
            loading = replies.isEmpty
            error = nil
            do {
                let replies = try await env.library.profileReplies(handle: handle)
                var likes: [String: Set<String>] = [:]
                var comments: [String: CommentIndex] = [:]
                for id in Set(replies.map(\.book.id)) {
                    likes[id] = (try? await env.library.likes(id: id)) ?? []
                    comments[id] = (try? await env.library.commentIndex(id: id)) ?? CommentIndex()
                }
                self.replies = replies
                self.likes = likes
                self.comments = comments
                loading = false
            } catch {
                loading = false
                self.error = error.localizedDescription
            }
        }
    }

    func toggleLike(bookId: String, postId: String) {
        Task {
            if let ids = try? await env.library.toggleLike(id: bookId, postId: postId) {
                likes[bookId] = ids
            }
        }
    }

    func onCommentsChanged(bookId: String, postId: String, comments: [PostComment]) {
        let current = self.comments[bookId] ?? CommentIndex()
        replies.removeAll { hit in
            hit.book.id == bookId && hit.post.id == postId && !comments.contains { $0.id == hit.comment.id }
        }
        self.comments[bookId] = current.afterThread(postId: postId, comments: comments)
    }
}

struct ProfileScreen: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Bindable var chrome: HideOnScrollState
    var displayName: String
    var onBack: () -> Void
    var onOpenProfile: (String, String) -> Void
    @State private var model: ProfileViewModel
    @State private var commentTarget: CommentTarget?
    @State private var shareText: String?
    @State private var lastOffset: CGFloat = 0

    init(env: AppEnvironment, handle: String, displayName: String, chrome: HideOnScrollState, onBack: @escaping () -> Void, onOpenProfile: @escaping (String, String) -> Void) {
        self.displayName = displayName
        self.chrome = chrome
        self.onBack = onBack
        self.onOpenProfile = onOpenProfile
        _model = State(initialValue: ProfileViewModel(env: env, handle: handle))
    }

    var body: some View {
        let me = app.readerIdentity
        let viewingSelf = model.handle.caseInsensitiveCompare(me.handle) == .orderedSame
        let name = viewingSelf ? me.name : displayName.ifBlank { model.handle }
        let handle = viewingSelf ? me.handle : model.handle
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
            }) {
                VStack(alignment: .leading, spacing: 0) {
                    Text(name).font(MewoFont.bold(15)).foregroundStyle(mewo.ink).lineLimit(1)
                    Text(model.replies.count == 1 ? "1 post" : "\(model.replies.count) posts")
                        .font(MewoFont.regular(13))
                        .foregroundStyle(mewo.mute)
                }
                .padding(.trailing, 16)
            }
            if model.loading {
                ProgressView().tint(mewo.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error = model.error {
                Text(error).font(MewoFont.regular(17)).foregroundStyle(mewo.like).padding(16)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        GeometryReader { geo in
                            Color.clear.preference(key: ScrollOffsetKey.self, value: -geo.frame(in: .named("profile")).minY)
                        }
                        .frame(height: 0)
                        header(name: name, handle: handle)
                        if model.replies.isEmpty {
                            Text(viewingSelf ? "Reply to a line. It shows up here." : "No posts yet.")
                                .font(MewoFont.regular(17))
                                .foregroundStyle(mewo.mute)
                                .padding(.horizontal, 16)
                                .padding(.vertical, 24)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        } else {
                            ForEach(model.replies, id: \.comment.id) { hit in
                                let identity = app.commenterIdentity(hit.comment)
                                replyRow(hit, identity: identity)
                            }
                        }
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
            model.refresh()
            chrome.showWhenIdle(model.loading || model.replies.isEmpty)
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
        ZStack(alignment: .topLeading) {
            VStack(alignment: .leading, spacing: 0) {
                mewo.hover.frame(height: 125)
                Spacer().frame(height: 42)
                Text(name).font(MewoFont.bold(20)).foregroundStyle(mewo.ink).padding(.horizontal, 16)
                Text("@\(handle)").font(MewoFont.regular(13)).foregroundStyle(mewo.mute).padding(.horizontal, 16)
                Spacer().frame(height: 16)
                VStack(spacing: 10) {
                    Text("Posts").font(MewoFont.bold(15)).foregroundStyle(mewo.ink)
                    Capsule().fill(mewo.accent).frame(width: 56, height: 3)
                }
                .frame(maxWidth: .infinity)
                .frame(height: 48)
                Hairline()
            }
            Circle()
                .fill(mewo.ground)
                .frame(width: 76, height: 76)
                .overlay(Avatar(name: name, size: 68))
                .padding(.leading, 12)
                .padding(.top, 87)
        }
    }

    private func replyRow(_ hit: ProfileReply, identity: ReaderIdentity) -> some View {
        VStack(spacing: 0) {
            ThreadPost(
                name: hit.book.author,
                handle: hit.book.handle,
                meta: hit.post.chapter,
                text: hit.post.text,
                heading: hit.post.kind == .heading,
                cover: app.library.coverFile(id: hit.book.id),
                connectDown: true
            )
            ThreadPost(
                name: identity.name,
                handle: identity.handle,
                meta: relativeTime(createdAt: hit.comment.createdAt),
                text: hit.comment.text,
                heading: false,
                cover: nil,
                connectDown: false,
                onOpenProfile: onOpenProfile
            )
            PostActions(
                liked: model.likes[hit.book.id]?.contains(hit.post.id) == true,
                commentCount: model.comments[hit.book.id]?.count(hit.post.id) ?? 0,
                commented: model.comments[hit.book.id]?.commented(hit.post.id) == true,
                onComment: { commentTarget = CommentTarget(book: hit.book, post: hit.post) },
                onLike: { model.toggleLike(bookId: hit.book.id, postId: hit.post.id) },
                onRepost: { shareText = quoteLine(author: hit.book.author, title: hit.book.title, text: hit.post.text) },
                onShare: { shareText = hit.post.text }
            )
            .padding(.leading, 68)
            .padding(.trailing, 12)
            Hairline()
        }
        .contentShape(Rectangle())
        .onTapGesture { commentTarget = CommentTarget(book: hit.book, post: hit.post) }
    }
}
