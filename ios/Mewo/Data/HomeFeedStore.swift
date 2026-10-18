import Foundation
import Observation

/// Home feed for one hosted account. The cached page paints first; a late page
/// from the previous account never replaces it.
@Observable
@MainActor
final class HomeFeedStore {
    private let sessions: HostedSessionStore
    private let client: HostedClient
    private let directory: URL
    private var epoch = 0
    /// Bumped when a refresh starts. A load-more that started earlier is dropped.
    private var refreshGeneration = 0
    private var flight: FeedFlight?
    private var more: FeedFlight?
    private(set) var loadState: FeedLoad = .signedOut

    init(sessions: HostedSessionStore, client: HostedClient, files: URL, refreshesOnInit: Bool) {
        self.sessions = sessions
        self.client = client
        self.directory = files.appendingPathComponent("hosted-feeds", isDirectory: true)
        sessions.addListener { [weak self] previous, next in
            self?.sessionChanged(from: previous, to: next)
        }
        restore()
        if refreshesOnInit, sessions.session.signedIn {
            Task { await self.load() }
        }
    }

    /// A pull waits on the refresh already running for this token.
    func load() async {
        let token = sessions.session.token
        if let flight, flight.token == token {
            await flight.task.value
            return
        }
        more?.task.cancel()
        more = nil
        refreshGeneration += 1
        let id = UUID()
        let generation = epoch
        let task = Task { @MainActor in
            defer {
                if self.flight?.id == id { self.flight = nil }
            }
            await self.loadBody(startedWith: token, generation: generation)
        }
        let previous = flight
        flight = FeedFlight(id: id, task: task, token: token)
        previous?.task.cancel()
        await task.value
    }

    func loadMore() async {
        guard more == nil, flight == nil else { return }
        guard case .loaded(let items, let refreshing, let cursor) = loadState,
              !refreshing, let cursor, !cursor.isEmpty else { return }
        let token = sessions.session.token
        let session = sessions.session
        let generation = epoch
        let refresh = refreshGeneration
        guard session.signedIn else { return }
        let id = UUID()
        let task = Task { @MainActor in
            defer {
                if self.more?.id == id { self.more = nil }
            }
            do {
                let page = try await self.fetch(session, cursor: cursor)
                guard !Task.isCancelled, generation == self.epoch, refresh == self.refreshGeneration else { return }
                guard self.sessions.session.token == token else { return }
                self.loadState = reduceFeedLoad(self.loadState, .pageAppended(page.items, page.nextCursor))
            } catch is CancellationError {
                return
            } catch {
                guard !Task.isCancelled, generation == self.epoch, refresh == self.refreshGeneration else { return }
                guard self.sessions.session.token == token else { return }
                let message = error.localizedDescription
                if case .loaded = self.loadState {
                    self.loadState = .failed(
                        message: message.isEmpty ? "Could not load the feed." : message,
                        items: items,
                        nextCursor: cursor
                    )
                }
            }
        }
        more = FeedFlight(id: id, task: task, token: token)
        await task.value
    }

    private func loadBody(startedWith token: String?, generation: Int) async {
        let session = sessions.session
        guard generation == epoch, session.token == token else { return }
        guard session.signedIn, let url = session.normalizedUrl(), let user = session.username, !user.isEmpty else {
            loadState = .signedOut
            return
        }
        if loadState.items.isEmpty, let cached = readFeed(url: url, username: user) {
            guard generation == epoch, sessions.session.token == token else { return }
            loadState = reduceFeedLoad(loadState, .cached(cached.items, cached.nextCursor))
        }
        guard generation == epoch, sessions.session.token == token else { return }
        loadState = reduceFeedLoad(loadState, .refreshStarted)
        do {
            let page = try await fetch(session, cursor: nil)
            guard generation == epoch, sessions.session.token == token else { return }
            loadState = reduceFeedLoad(loadState, .refreshSucceeded(page.items, page.nextCursor))
            writeFeed(url: url, username: user, page: page)
        } catch is CancellationError {
            return
        } catch let urlError as URLError where urlError.code == .cancelled {
            return
        } catch {
            guard generation == epoch, sessions.session.token == token else { return }
            let message = error.localizedDescription
            loadState = reduceFeedLoad(loadState, .refreshFailed(message.isEmpty ? "Could not load the feed." : message))
        }
    }

    private func fetch(_ session: HostedSession, cursor: String?) async throws -> HomeFeedPage {
        do {
            return try await client.feed(session, cursor: cursor)
        } catch let err as HostedException where err.code == 401 {
            if sessions.session.token == session.token {
                sessions.signOut()
            }
            throw err
        }
    }

    private func sessionChanged(from previous: HostedSession, to next: HostedSession) {
        let urlChanged = previous.normalizedUrl() != next.normalizedUrl()
        let userChanged = previous.username != next.username
        let signedOut = previous.signedIn && !next.signedIn
        let tokenChanged = previous.token != next.token
        guard tokenChanged || (previous.signedIn && (urlChanged || userChanged)) else { return }
        epoch += 1
        refreshGeneration += 1
        flight?.task.cancel()
        flight = nil
        more?.task.cancel()
        more = nil
        if previous.signedIn && (signedOut || urlChanged || userChanged) {
            clear(previous)
        }
        if next.signedIn {
            restore()
            Task { await self.load() }
        } else {
            loadState = .signedOut
        }
    }

    private func restore() {
        let session = sessions.session
        guard session.signedIn, let url = session.normalizedUrl(), let user = session.username, !user.isEmpty else {
            loadState = .signedOut
            return
        }
        if let cached = readFeed(url: url, username: user) {
            loadState = .loaded(items: cached.items, refreshing: true, nextCursor: cached.nextCursor)
        } else {
            loadState = .loading
        }
    }

    private func clear(_ session: HostedSession) {
        guard let url = session.normalizedUrl(), let user = session.username else { return }
        try? FileManager.default.removeItem(at: feedFile(url, user))
    }

    private func writeFeed(url: String, username: String, page: HomeFeedPage) {
        let dest = feedFile(url, username)
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            let tmp = dest.appendingPathExtension("tmp")
            try JSONFiles.write(page, to: tmp)
            if FileManager.default.fileExists(atPath: dest.path) {
                try FileManager.default.removeItem(at: dest)
            }
            try FileManager.default.moveItem(at: tmp, to: dest)
        } catch {
            // A full disk must not turn a fetched feed into an error.
        }
    }

    private func readFeed(url: String, username: String) -> HomeFeedPage? {
        JSONFiles.read(HomeFeedPage.self, from: feedFile(url, username))
    }

    private func feedFile(_ url: String, _ username: String) -> URL {
        directory.appendingPathComponent(shelfCacheKey(serverUrl: url, username: username) + ".json")
    }
}

private struct FeedFlight {
    var id: UUID
    var task: Task<Void, Never>
    var token: String?
}
