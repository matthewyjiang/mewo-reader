import SwiftUI

struct TimelinePost: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.readerScale) private var scale
    var post: FeedPost
    var author: String
    var handle: String
    var cover: URL?
    var liked: Bool
    var commentCount: Int = 0
    var commented: Bool = false
    var onComment: () -> Void
    var onLike: () -> Void
    var onRepost: () -> Void
    var onShare: () -> Void
    var onOpen: (() -> Void)?
    var onChapterClick: (() -> Void)?

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .top, spacing: 12) {
                Avatar(name: author, cover: cover, size: 40)
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Text(author)
                            .font(MewoFont.bold(15))
                            .lineLimit(1)
                        chapterLine
                    }
                    Text(post.text)
                        .font(post.kind == .heading ? MewoFont.bold(scale.headingSize) : MewoFont.regular(scale.bodySize))
                        .lineSpacing(max(0, (post.kind == .heading ? scale.headingLine : scale.bodyLine) - (post.kind == .heading ? scale.headingSize : scale.bodySize)))
                        .foregroundStyle(mewo.ink)
                    PostActions(
                        liked: liked,
                        commentCount: commentCount,
                        commented: commented,
                        onComment: onComment,
                        onLike: onLike,
                        onRepost: onRepost,
                        onShare: onShare
                    )
                }
            }
            .padding(.leading, 16)
            .padding(.trailing, 12)
            .padding(.top, 12)
            .contentShape(Rectangle())
            .onTapGesture { onOpen?() }
            Hairline()
        }
        .foregroundStyle(mewo.ink)
        .accessibilityIdentifier("post-\(post.id)")
        .accessibilityElement(children: .contain)
    }

    private var chapterLine: some View {
        Text("@\(handle) · \(post.chapter)")
            .font(MewoFont.regular(13))
            .foregroundStyle(mewo.mute)
            .lineLimit(1)
            .onTapGesture { onChapterClick?() }
            .accessibilityLabel("Chapters, \(post.chapter)")
    }
}

struct PostActions: View {
    @Environment(\.mewo) private var mewo
    var liked: Bool
    var commentCount: Int
    var commented: Bool
    var onComment: () -> Void
    var onLike: () -> Void
    var onRepost: () -> Void
    var onShare: () -> Void

    var body: some View {
        HStack {
            action(
                icon: "bubble.right",
                tint: commented ? mewo.accent : mewo.mute,
                label: commentCount == 0 ? "Comment on this line" : "Comments, \(commentCount)",
                count: commentCount,
                action: onComment
            )
            action(icon: "arrow.2.squarepath", tint: mewo.mute, label: "Quote this line", action: onRepost)
            action(
                icon: liked ? "heart.fill" : "heart",
                tint: liked ? mewo.like : mewo.mute,
                label: liked ? "Unlike" : "Like",
                scale: liked ? 1.18 : 1,
                action: onLike
            )
            action(icon: "square.and.arrow.up", tint: mewo.mute, label: "Share", action: onShare)
        }
    }

    private func action(icon: String, tint: Color, label: String, count: Int = 0, scale: CGFloat = 1, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 4) {
                Image(systemName: icon)
                    .font(.system(size: 16))
                    .scaleEffect(scale)
                if count > 0 {
                    Text("\(count)").font(MewoFont.regular(13))
                }
            }
            .foregroundStyle(tint)
            .frame(minWidth: count > 0 ? 52 : 40, minHeight: 40)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }
}

struct ShareSheet: UIViewControllerRepresentable {
    var text: String

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: [text], applicationActivities: nil)
    }

    func updateUIViewController(_ uiViewController: UIActivityViewController, context: Context) {}
}
