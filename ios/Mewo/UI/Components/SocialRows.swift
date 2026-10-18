import SwiftUI

struct RepostContext: View {
    @Environment(\.mewo) private var mewo
    var username: String
    var onOpenActor: () -> Void

    var body: some View {
        Button(action: onOpenActor) {
            HStack(spacing: 8) {
                Image(systemName: "arrow.2.squarepath")
                    .font(.system(size: 16))
                    .frame(width: 16, height: 16)
                Text("\(username) reposted")
                    .font(MewoFont.bold(13))
            }
            .foregroundStyle(mewo.mute)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.leading, 68)
            .padding(.trailing, 16)
            .padding(.top, 8)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(username) reposted")
    }
}

/// A comment on Home and Profile: the passage keeps its actions and an
/// "Open line in book" link, and the thread line runs down to the reply.
struct PassageReply: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.readerScale) private var scale
    var passageName: String
    var passageHandle: String
    var passageMeta: String
    var passageText: String
    var passageHeading: Bool
    var passageCover: URL?
    var actorName: String
    var actorHandle: String
    var actorMeta: String
    var actorText: String
    var liked: Bool
    var commentCount: Int
    var commented: Bool
    var reposted: Bool
    var repostCount: Int
    var onOpenPassage: () -> Void
    var onOpenReply: () -> Void
    var onOpenActor: () -> Void
    var onComment: () -> Void
    var onLike: () -> Void
    var onRepost: () -> Void
    var onQuote: () -> Void
    var onShare: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            passage
            ThreadPost(
                name: actorName,
                handle: actorHandle,
                meta: actorMeta,
                text: actorText,
                heading: false,
                cover: nil,
                connectDown: false,
                onOpenProfile: { _, _ in onOpenActor() },
                onOpen: onOpenReply,
                trailing: 12
            )
            Hairline()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// Same insets as a normal post: 16 leading, 12 trailing, 12 top. The thread
    /// line fills this row so it meets the reply under the passage actions.
    private var passage: some View {
        HStack(alignment: .top, spacing: 12) {
            Color.clear.frame(width: 40, height: 40)
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 6) {
                    Text(passageName)
                        .font(MewoFont.bold(15))
                        .lineLimit(1)
                    Text("@\(passageHandle) · \(passageMeta)")
                        .font(MewoFont.regular(13))
                        .foregroundStyle(mewo.mute)
                        .lineLimit(1)
                }
                Text(passageText)
                    .font(passageHeading ? MewoFont.bold(scale.headingSize) : MewoFont.regular(scale.bodySize))
                    .lineSpacing(max(0, (passageHeading ? scale.headingLine : scale.bodyLine) - (passageHeading ? scale.headingSize : scale.bodySize)))
                    .foregroundStyle(mewo.ink)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.top, 4)
                    .contentShape(Rectangle())
                    .onTapGesture(perform: onOpenPassage)
                PostActions(
                    liked: liked,
                    commentCount: commentCount,
                    commented: commented,
                    reposted: reposted,
                    repostCount: repostCount,
                    onComment: onComment,
                    onLike: onLike,
                    onRepost: onRepost,
                    onQuote: onQuote,
                    onShare: onShare
                )
                .padding(.top, 4)
                Button(action: onOpenPassage) {
                    Text("Open line in book")
                        .font(MewoFont.regular(13))
                        .foregroundStyle(mewo.accent)
                        .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                        .padding(.horizontal, 8)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.leading, 16)
        .padding(.trailing, 12)
        .padding(.top, 12)
        .background(alignment: .topLeading) {
            VStack(spacing: 0) {
                Avatar(name: passageName, cover: passageCover, size: 40)
                mewo.line
                    .frame(width: 2)
                    .frame(maxHeight: .infinity)
                    .padding(.top, 4)
            }
            .padding(.leading, 16)
            .padding(.top, 12)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
    }
}
