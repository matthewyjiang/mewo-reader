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

struct PassageReply: View {
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
    var onOpenActor: () -> Void
    var onComment: () -> Void
    var onLike: () -> Void
    var onRepost: () -> Void
    var onQuote: () -> Void
    var onShare: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            ThreadPost(
                name: passageName,
                handle: passageHandle,
                meta: passageMeta,
                text: passageText,
                heading: passageHeading,
                cover: passageCover,
                connectDown: true
            )
            .contentShape(Rectangle())
            .onTapGesture(perform: onOpenPassage)
            ThreadPost(
                name: actorName,
                handle: actorHandle,
                meta: actorMeta,
                text: actorText,
                heading: false,
                cover: nil,
                connectDown: false,
                onOpenProfile: { _, _ in onOpenActor() }
            )
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
            .padding(.leading, 68)
            .padding(.trailing, 12)
            Hairline()
        }
    }
}
