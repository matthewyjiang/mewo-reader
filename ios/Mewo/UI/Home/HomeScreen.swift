import SwiftUI

enum HomeSection: String, CaseIterable, Identifiable {
    case feed, books

    var id: String { rawValue }

    var title: String {
        self == .feed ? "Feed" : "Books"
    }
}

/// The feed is server-only. Local mode opens on the shelf.
func homeTabs(hosted: Bool) -> [HomeSection] {
    hosted ? [.feed, .books] : [.books]
}

struct HomeScreen: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Bindable var chrome: HideOnScrollState
    var onOpenBook: (String) -> Void
    var onOpenPost: (String, String) -> Void
    var onOpenAccount: () -> Void
    var onOpenProfile: (String, String) -> Void
    var onBrowseCatalog: () -> Void = {}
    @State private var section: HomeSection = .feed

    var body: some View {
        let hosted = app.backend.kind == .hosted
        let sections = homeTabs(hosted: hosted)
        let selected = hosted ? section : HomeSection.books
        VStack(spacing: 0) {
            MewoAppBar(visible: chrome.visible, leading: { ProfileButton(onClick: onOpenAccount) }, center: { BrandMark() })
            UnderlineTabs(
                items: sections.map { UnderlineTabItem(id: $0.rawValue, title: $0.title) },
                selected: selected.rawValue,
                idPrefix: "home",
                onSelect: { id in
                    guard hosted, let next = HomeSection(rawValue: id) else { return }
                    chrome.show()
                    section = next
                }
            )
            switch selected {
            case .feed:
                FeedScreen(
                    env: app,
                    chrome: chrome,
                    onOpenPost: onOpenPost,
                    onOpenProfile: onOpenProfile
                )
            case .books:
                LibraryScreen(
                    env: app,
                    chrome: chrome,
                    onOpenBook: onOpenBook,
                    onOpenAccount: onOpenAccount,
                    onBrowseCatalog: onBrowseCatalog,
                    showsBar: false
                )
            }
        }
        .background(mewo.ground)
    }
}

@Observable
@MainActor
final class FeedRowSocial {
    private var socials: [String: BookSocial] = [:]
    private let library: LibraryStore

    init(library: LibraryStore) { self.library = library }

    func social(for bookId: String) -> BookSocial {
        if let social = socials[bookId] { return social }
        let social = BookSocial(id: bookId, library: library)
        socials[bookId] = social
        return social
    }

    func refresh() {
        for social in socials.values { social.invalidate() }
        for social in socials.values { social.load() }
    }
}

struct FeedScreen: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app
    @Bindable var chrome: HideOnScrollState
    var onOpenPost: (String, String) -> Void
    var onOpenProfile: (String, String) -> Void
    @State private var social: FeedRowSocial
    @State private var commentTarget: CommentTarget?
    @State private var shareText: String?
    @State private var lastOffset: CGFloat = 0

    init(
        env: AppEnvironment,
        chrome: HideOnScrollState,
        onOpenPost: @escaping (String, String) -> Void,
        onOpenProfile: @escaping (String, String) -> Void
    ) {
        self.chrome = chrome
        self.onOpenPost = onOpenPost
        self.onOpenProfile = onOpenProfile
        _social = State(initialValue: FeedRowSocial(library: env.library))
    }

    var body: some View {
        let hosted = app.backend.kind == .hosted
        let load = hosted ? app.homeFeed.loadState : FeedLoad.loaded(items: [], refreshing: false, nextCursor: nil)
        Group {
            if !hosted {
                EmptyView()
            } else if case .signedOut = load {
                Text(hostedNotSignedIn)
                    .font(MewoFont.bold(22))
                    .foregroundStyle(mewo.ink)
                    .padding(.horizontal, 28)
                    .padding(.vertical, 48)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    .accessibilityIdentifier("feed-signed-out")
            } else if load.showEmpty {
                Text(emptyFeedCopy)
                    .font(MewoFont.bold(22))
                    .foregroundStyle(mewo.ink)
                    .padding(.horizontal, 28)
                    .padding(.vertical, 48)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            } else if load.isFailedEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Couldn't load")
                        .font(MewoFont.bold(22))
                        .foregroundStyle(mewo.ink)
                    if case .failed(let message, _, _) = load {
                        Text(message).font(MewoFont.regular(15)).foregroundStyle(mewo.mute)
                    }
                    Button("Try again") { Task { await app.homeFeed.load() } }
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.accent)
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("feed-retry")
                }
                .padding(.horizontal, 28)
                .padding(.vertical, 48)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            } else if case .loading = load {
                ProgressView()
                    .tint(mewo.accent)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                    .padding(.top, 24)
                    .accessibilityIdentifier("feed-loading")
            } else if load.items.isEmpty {
                Color.clear
            } else {
                feedList(load)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .onAppear {
            social.refresh()
            if hosted { Task { await app.homeFeed.load() } }
        }
        .onChange(of: app.hostedSession.session.token) { _, _ in
            social = FeedRowSocial(library: app.library)
        }
        .sheet(item: $commentTarget) { target in
            CommentSheet(
                target: target,
                publicNotes: hosted,
                onThreadChanged: { postId, comments in
                    social.social(for: target.book.id).threadChanged(postId: postId, comments: comments)
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

    private func feedList(_ load: FeedLoad) -> some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                GeometryReader { geo in
                    Color.clear.preference(key: ScrollOffsetKey.self, value: -geo.frame(in: .named("feed")).minY)
                }
                .frame(height: 0)
                if let message = load.failureMessage {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(message).font(MewoFont.regular(15)).foregroundStyle(mewo.mute)
                        Button("Try again") { Task { await app.homeFeed.load() } }
                            .font(MewoFont.regular(15))
                            .foregroundStyle(mewo.accent)
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("feed-retry")
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                }
                ForEach(load.items) { item in
                    row(item)
                        .onAppear {
                            social.social(for: item.book.id).load()
                            if item.id == load.items.last?.id {
                                Task { await app.homeFeed.loadMore() }
                            }
                        }
                }
            }
        }
        .coordinateSpace(name: "feed")
        .refreshable { await app.homeFeed.load() }
        .scrollBounceBehavior(.always)
        .scrollIndicators(.hidden)
        .accessibilityIdentifier("feed-scroll")
        .onPreferenceChange(ScrollOffsetKey.self) { offset in
            chrome.ingest(delta: offset - lastOffset)
            lastOffset = offset
            chrome.bindTop(offset)
        }
    }

    @ViewBuilder
    private func row(_ item: HomeFeedItem) -> some View {
        let bookSocial = social.social(for: item.book.id)
        if item.isRepost {
            VStack(alignment: .leading, spacing: 0) {
                RepostContext(username: item.actor) {
                    onOpenProfile(item.actor, item.actor)
                }
                passage(item, social: bookSocial)
            }
        } else if let comment = item.comment {
            PassageReply(
                passageName: item.book.author,
                passageHandle: item.book.handle,
                passageMeta: item.post.chapter,
                passageText: item.post.text,
                passageHeading: item.post.kind == .heading,
                passageCover: app.library.coverFile(id: item.book.id),
                actorName: comment.username,
                actorHandle: comment.username,
                actorMeta: relativeTime(createdAt: comment.createdAt),
                actorText: comment.text,
                liked: bookSocial.likes?.contains(item.post.id) == true,
                commentCount: bookSocial.comments?.count(item.post.id) ?? 0,
                commented: bookSocial.comments?.commented(item.post.id) == true,
                reposted: bookSocial.comments?.didRepost(item.post.id) == true,
                repostCount: bookSocial.comments?.repostCount(item.post.id) ?? 0,
                onOpenPassage: { onOpenPost(item.book.id, item.post.id) },
                onOpenReply: { commentTarget = CommentTarget(book: item.book, post: item.post) },
                onOpenActor: { onOpenProfile(comment.username, comment.username) },
                onComment: { commentTarget = CommentTarget(book: item.book, post: item.post) },
                onLike: { bookSocial.toggleLike(postId: item.post.id) },
                onRepost: { bookSocial.toggleRepost(postId: item.post.id) },
                onQuote: { shareText = quoteLine(author: item.book.author, title: item.book.title, text: item.post.text) },
                onShare: { shareText = item.post.text }
            )
        }
    }

    private func passage(_ item: HomeFeedItem, social: BookSocial) -> some View {
        TimelinePost(
            post: item.post,
            author: item.book.author,
            handle: item.book.handle,
            cover: app.library.coverFile(id: item.book.id),
            liked: social.likes?.contains(item.post.id) == true,
            commentCount: social.comments?.count(item.post.id) ?? 0,
            commented: social.comments?.commented(item.post.id) == true,
            reposted: social.comments?.didRepost(item.post.id) == true,
            repostCount: social.comments?.repostCount(item.post.id) ?? 0,
            onComment: { commentTarget = CommentTarget(book: item.book, post: item.post) },
            onLike: { social.toggleLike(postId: item.post.id) },
            onRepost: { social.toggleRepost(postId: item.post.id) },
            onQuote: { shareText = quoteLine(author: item.book.author, title: item.book.title, text: item.post.text) },
            onShare: { shareText = item.post.text },
            onOpen: { onOpenPost(item.book.id, item.post.id) }
        )
    }
}
