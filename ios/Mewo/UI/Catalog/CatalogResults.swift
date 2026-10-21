import SwiftUI

struct CatalogResults: View {
    @Environment(\.mewo) private var mewo
    @Bindable var feed: CatalogFeed
    var onOpenBook: (String) -> Void

    var body: some View {
        Group {
            if let failure = feed.failure {
                failureBlock(failure)
            } else if feed.items.isEmpty, case .loading = feed.phase {
                ProgressView()
                    .tint(mewo.accent)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                    .padding(.top, 24)
                    .accessibilityIdentifier("catalog-loading")
            } else if feed.items.isEmpty, case .loaded = feed.phase {
                VStack(alignment: .leading, spacing: 0) {
                    if let heading = catalogHeading(feed.query) {
                        Text(heading)
                            .font(MewoFont.bold(15))
                            .foregroundStyle(mewo.ink)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .frame(minHeight: 44)
                            .padding(.horizontal, 16)
                            .accessibilityIdentifier("catalog-heading")
                        Hairline()
                    }
                    Text(catalogEmptyCopy(feed.query))
                        .font(MewoFont.bold(22))
                        .foregroundStyle(mewo.ink)
                        .padding(.horizontal, 28)
                        .padding(.vertical, 48)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .accessibilityIdentifier("catalog-empty")
            } else if feed.items.isEmpty {
                Color.clear
            } else {
                list
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .sheet(item: Binding(
            get: { feed.sheet },
            set: { if $0 == nil { feed.close() } }
        )) { model in
            CatalogSheet(model: model) { id in
                feed.close()
                onOpenBook(id)
            }
            .presentationDetents([.large])
        }
    }

    private var list: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if let heading = catalogHeading(feed.query) {
                    Text(heading)
                        .font(MewoFont.bold(15))
                        .foregroundStyle(mewo.ink)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .frame(minHeight: 44)
                        .padding(.horizontal, 16)
                        .accessibilityIdentifier("catalog-heading")
                    Hairline()
                }
                if case .loading = feed.phase {
                    ProgressView()
                        .tint(mewo.accent)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 12)
                        .accessibilityIdentifier("catalog-loading")
                }
                if let notice = feed.notice {
                    Text(notice)
                        .font(MewoFont.regular(15))
                        .foregroundStyle(mewo.ink)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 8)
                        .accessibilityIdentifier("catalog-notice")
                }
                ForEach(feed.items) { work in
                    CatalogRow(
                        work: work,
                        request: feed.shown(work),
                        onOpen: { feed.open(work) },
                        onAct: {
                            Task {
                                let outcome = await feed.act(work)
                                if case .openBook(let id) = outcome { onOpenBook(id) }
                            }
                        }
                    )
                    .onAppear {
                        if work.id == feed.items.last?.id { feed.loadMore() }
                    }
                    Hairline()
                }
                if case .loaded(_, true) = feed.phase {
                    ProgressView()
                        .tint(mewo.accent)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 12)
                        .accessibilityLabel("Loading more")
                }
            }
        }
        .accessibilityIdentifier("catalog-results")
    }

    private func failureBlock(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if let heading = catalogHeading(feed.query) {
                Text(heading)
                    .font(MewoFont.bold(15))
                    .foregroundStyle(mewo.ink)
            }
            Text(message)
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.ink)
                .accessibilityIdentifier("catalog-error")
            Button("Try again") { feed.retry() }
                .font(MewoFont.regular(15))
                .foregroundStyle(mewo.accent)
                .buttonStyle(.plain)
                .accessibilityIdentifier("catalog-feed-retry")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}
