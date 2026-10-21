import Foundation
import Observation

@MainActor
protocol CatalogService: AnyObject {
    var access: CatalogAccess { get }
    var token: String? { get }
    func refreshAccess() async
    func page(query: String?, page: Int) async throws -> CatalogPage
    func work(workKey: String) async throws -> WorkDetail
    func request(_ body: NewBookRequest) async throws -> BookRequest
    func cancel(id: String) async throws
    func retry(id: String) async throws -> BookRequest
}

@Observable
@MainActor
final class HostedCatalogService: CatalogService {
    private(set) var access: CatalogAccess = .unknown
    var token: String? { sessions.session.token }

    private let sessions: HostedSessionStore
    private let client: HostedClient
    private let backend: BackendStore

    init(sessions: HostedSessionStore, client: HostedClient, backend: BackendStore) {
        self.sessions = sessions
        self.client = client
        self.backend = backend
        watch()
        Task { await refreshAccess() }
    }

    func refreshAccess() async {
        let session = sessions.session
        if backend.kind != .hosted || !session.signedIn {
            access = .off
            return
        }
        do {
            let me = try await run(session) { try await self.client.me($0) }
            guard sessions.session.token == session.token, backend.kind == .hosted else { return }
            access = me.canRequest ? .on : .off
        } catch let error as CatalogError where error.reason == .forbidden || error.reason == .unauthorized {
            access = .off
        } catch {
            // A blip keeps the last answer. The next foreground pass asks again.
        }
    }

    func page(query: String?, page: Int) async throws -> CatalogPage {
        let sent: String?
        if let query, case .search(let text) = catalogQuery(query) {
            sent = text
        } else {
            sent = nil
        }
        let session = try requireSession()
        return try await run(session) { try await self.client.catalog($0, query: sent, page: page) }
    }

    func work(workKey: String) async throws -> WorkDetail {
        let olid = catalogOlid(workKey)
        let session = try requireSession()
        return try await run(session) { try await self.client.catalogWork($0, olid: olid) }
    }

    func request(_ body: NewBookRequest) async throws -> BookRequest {
        let session = try requireSession()
        return try await run(session) { try await self.client.requestBook($0, body: body) }
    }

    func cancel(id: String) async throws {
        let session = try requireSession()
        try await run(session) { try await self.client.cancelRequest($0, id: id) }
    }

    func retry(id: String) async throws -> BookRequest {
        let session = try requireSession()
        return try await run(session) { try await self.client.retryRequest($0, id: id) }
    }

    private func requireSession() throws -> HostedSession {
        let session = sessions.session
        if backend.kind != .hosted || !session.signedIn {
            access = .off
            throw CatalogError(reason: .off, message: "Requests are off.")
        }
        return session
    }

    private func run<T>(_ session: HostedSession, _ block: (HostedSession) async throws -> T) async throws -> T {
        do {
            return try await expireOn401(sessions: sessions, session: session, operation: { try await block(session) })
        } catch {
            let catalog = catalogError(from: error)
            if catalog.reason == .forbidden || catalog.reason == .unauthorized {
                access = .off
            }
            throw catalog
        }
    }

    private func watch() {
        withObservationTracking { [weak self] in
            guard let self else { return }
            _ = self.sessions.session.token
            _ = self.backend.kind
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                await self.refreshAccess()
                self.watch()
            }
        }
    }
}
