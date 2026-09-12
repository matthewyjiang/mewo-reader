import SwiftUI

struct CommentTarget: Identifiable, Equatable {
    var book: BookRecord
    var post: FeedPost
    var id: String { "\(book.id):\(post.id)" }
}

struct CommentSheet: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Environment(\.readerScale) private var scale
    var target: CommentTarget
    var publicNotes: Bool
    var onThreadChanged: (String, [PostComment]) -> Void
    var onDismiss: () -> Void
    var onOpenProfile: (String, String) -> Void

    @State private var comments: [PostComment] = []
    @State private var draft = ""
    @State private var sending = false
    @State private var error: String?
    @State private var focused = false
    @FocusState private var fieldFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            composerBar
            ScrollView {
                VStack(spacing: 0) {
                    ThreadPost(
                        name: target.book.author,
                        handle: target.book.handle,
                        meta: target.post.chapter,
                        text: target.post.text,
                        heading: target.post.kind == .heading,
                        cover: app.library.coverFile(id: target.book.id),
                        connectDown: true
                    )
                    ForEach(comments) { comment in
                        let identity = app.commenterIdentity(comment)
                        ThreadPost(
                            name: identity.name,
                            handle: identity.handle,
                            meta: relativeTime(createdAt: comment.createdAt),
                            text: comment.text,
                            heading: false,
                            cover: nil,
                            connectDown: true,
                            onOpenProfile: onOpenProfile,
                            onDelete: comment.mine ? { delete(comment) } : nil
                        )
                    }
                    compose
                }
            }
        }
        .background(mewo.ground)
        .task(id: target.id) { await load() }
    }

    private var composerBar: some View {
        VStack(spacing: 0) {
            HStack {
                Button(action: onDismiss) {
                    Image(systemName: "xmark")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(mewo.ink)
                        .frame(width: 44, height: 56)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close")
                Spacer()
                PillButton(title: "Reply", enabled: !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !sending, action: postReply)
                    .padding(.trailing, 16)
            }
            Hairline()
        }
    }

    private var compose: some View {
        HStack(alignment: .top, spacing: 12) {
            Avatar(name: app.readerIdentity.name, size: 40)
                .onTapGesture { onOpenProfile(app.readerIdentity.handle, app.readerIdentity.name) }
            VStack(alignment: .leading, spacing: 4) {
                if focused || !draft.isEmpty {
                    Text("Replying to @\(target.book.handle)")
                        .font(MewoFont.regular(13))
                        .foregroundStyle(mewo.accent)
                }
                TextField("Post your reply", text: $draft, axis: .vertical)
                    .font(MewoFont.regular(17))
                    .foregroundStyle(mewo.ink)
                    .focused($fieldFocused)
                    .disabled(sending)
                    .onChange(of: fieldFocused) { _, on in focused = on }
                    .accessibilityLabel(publicNotes ? "Reply, public on this server" : "Reply")
                    .accessibilityIdentifier("reply-field")
                if let error {
                    Text(error).font(MewoFont.regular(15)).foregroundStyle(mewo.like)
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 4)
        .padding(.bottom, 12)
    }

    private func load() async {
        error = nil
        do {
            let next = try await app.library.comments(id: target.book.id, postId: target.post.id)
            comments = next
            onThreadChanged(target.post.id, next)
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func postReply() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !sending else { return }
        sending = true
        Task {
            do {
                let next = try await app.library.addComment(id: target.book.id, postId: target.post.id, text: text)
                draft = ""
                focused = false
                comments = next
                onThreadChanged(target.post.id, next)
            } catch {
                self.error = error.localizedDescription
            }
            sending = false
        }
    }

    private func delete(_ comment: PostComment) {
        Task {
            do {
                let next = try await app.library.deleteComment(id: target.book.id, postId: target.post.id, commentId: comment.id)
                comments = next
                onThreadChanged(target.post.id, next)
            } catch {
                self.error = error.localizedDescription
            }
        }
    }
}

struct ThreadPost: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.readerScale) private var scale
    var name: String
    var handle: String
    var meta: String
    var text: String
    var heading: Bool
    var cover: URL?
    var connectDown: Bool
    var onOpenProfile: ((String, String) -> Void)?
    var onDelete: (() -> Void)?

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(spacing: 4) {
                Avatar(name: name, cover: cover, size: 40)
                    .onTapGesture { onOpenProfile?(handle, name) }
                if connectDown {
                    mewo.line.frame(width: 2).frame(maxHeight: .infinity)
                }
            }
            .frame(width: 40)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 4) {
                    Text(name)
                        .font(MewoFont.bold(15))
                        .lineLimit(1)
                        .onTapGesture { onOpenProfile?(handle, name) }
                    Text("@\(handle) · \(meta)")
                        .font(MewoFont.regular(13))
                        .foregroundStyle(mewo.mute)
                        .lineLimit(1)
                        .onTapGesture { onOpenProfile?(handle, name) }
                    if onDelete != nil {
                        Spacer(minLength: 0)
                        Menu {
                            Button("Delete", role: .destructive) { onDelete?() }
                        } label: {
                            Image(systemName: "ellipsis")
                                .foregroundStyle(mewo.mute)
                                .frame(width: 40, height: 40)
                        }
                        .accessibilityLabel("More")
                    }
                }
                Text(text)
                    .font(heading ? MewoFont.bold(scale.headingSize) : MewoFont.regular(scale.bodySize))
                    .foregroundStyle(mewo.ink)
            }
            .padding(.bottom, 12)
        }
        .padding(.leading, 16)
        .padding(.trailing, 4)
        .padding(.top, 12)
    }
}

struct ChapterSheet: View {
    @Environment(\.mewo) private var mewo
    var title: String
    var headings: [FeedPost]
    var currentIndex: Int
    var onJump: (FeedPost) -> Void
    var onDismiss: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(MewoFont.bold(22))
                .foregroundStyle(mewo.ink)
                .padding(.horizontal, 16)
                .padding(.bottom, 8)
                .accessibilityIdentifier("chapter-sheet")
            if headings.isEmpty {
                Text("This book has no chapter headings.")
                    .font(MewoFont.regular(17))
                    .foregroundStyle(mewo.mute)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(Array(headings.enumerated()), id: \.element.id) { index, post in
                                let current = index == currentIndex
                                Button {
                                    onJump(post)
                                } label: {
                                    Text(post.text)
                                        .font(current ? MewoFont.bold(17) : MewoFont.regular(17))
                                        .foregroundStyle(current ? mewo.ink : mewo.mute)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .padding(.horizontal, 16)
                                        .padding(.vertical, 14)
                                        .background(current ? mewo.hover : mewo.ground)
                                }
                                .buttonStyle(.plain)
                                .id(post.id)
                                Hairline()
                            }
                        }
                    }
                    .onAppear {
                        if headings.indices.contains(currentIndex) {
                            proxy.scrollTo(headings[currentIndex].id, anchor: .top)
                        }
                    }
                }
            }
        }
        .padding(.bottom, 28)
        .background(mewo.ground)
        .presentationDetents([.medium, .large])
    }
}
