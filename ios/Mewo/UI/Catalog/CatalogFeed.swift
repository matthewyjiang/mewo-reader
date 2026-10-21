import Foundation
import Observation

enum CatalogPhase: Equatable, Sendable {
    case idle
    case loading(CatalogPage?)
    case loaded(CatalogPage, more: Bool)
    case failed(String)
}

enum RowOutcome: Equatable, Sendable {
    case done
    case openBook(String)
}

/// Catalog tab. Trending is remembered per token. A memo built from more than
/// one page stays put; a single page is shown and quietly fetched again.
@Observable
@MainActor
final class CatalogFeed {
    private(set) var phase: CatalogPhase = .idle
    private(set) var overlay: [String: BookRequest?] = [:]
    var sheet: CatalogSheetModel?
    var notice: String?
    private(set) var query = ""

    @ObservationIgnored private(set) var loadTask: Task<Void, Never>?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var visible = false
    @ObservationIgnored private var sessionSeen: String?
    @ObservationIgnored private var trending: [String: TrendingMemo] = [:]
    @ObservationIgnored private let service: CatalogService
    @ObservationIgnored private let onImported: () -> Void
    @ObservationIgnored private let debounceNanos: UInt64
    @ObservationIgnored private let pollNanos: UInt64

    init(
        service: CatalogService,
        onImported: @escaping () -> Void = {},
        debounceNanos: UInt64 = catalogDebounceNanos,
        pollNanos: UInt64 = catalogPollNanos
    ) {
        self.service = service
        self.onImported = onImported
        self.debounceNanos = debounceNanos
        self.pollNanos = pollNanos
        self.sessionSeen = service.token
    }

    var items: [CatalogWork] {
        switch phase {
        case .loaded(let page, _): return page.items
        case .loading(let page): return page?.items ?? []
        case .idle, .failed: return []
        }
    }

    var failure: String? {
        if case .failed(let message) = phase { return message }
        return nil
    }

    func shown(_ work: CatalogWork) -> BookRequest? {
        switch overlay[work.workKey] {
        case .some(let request): return request
        case .none: return work.request
        }
    }

    func setQuery(_ query: String) {
        let previous = catalogQuery(self.query)
        let next = catalogQuery(query)
        self.query = query
        guard previous != next else { return }
        overlay.removeAll()
        notice = nil
        close()
        guard visible else { return }
        start(append: false, debounce: true, quietMemo: false)
    }

    func show() {
        let opening = !visible
        visible = true
        guard opening else { return }
        start(append: false, debounce: false, quietMemo: true)
    }

    func hide() {
        guard visible else { return }
        visible = false
        loadTask?.cancel()
        loadTask = nil
        generation += 1
        close()
    }

    func loadMore() {
        guard visible, case .loaded(let page, false) = phase, page.nextPage != nil else { return }
        start(append: true, debounce: false, quietMemo: false)
    }

    func retry() {
        guard visible, case .failed = phase else { return }
        notice = nil
        start(append: false, debounce: false, quietMemo: false)
    }

    func noteSession(_ token: String?) {
        if token == sessionSeen { return }
        sessionSeen = token
        loadTask?.cancel()
        loadTask = nil
        generation += 1
        overlay.removeAll()
        close()
        phase = .idle
        guard visible, token != nil else { return }
        start(append: false, debounce: false, quietMemo: true)
    }

    func act(_ work: CatalogWork) async -> RowOutcome {
        let current = shown(work)
        switch rowAction(current, work.onServer) {
        case .pending:
            return .done
        case .open:
            if let id = current?.bookId ?? work.onServer?.bookId {
                return .openBook(id)
            }
            return .done
        case .request, .again:
            await send(work.workKey) { try await self.service.request(NewBookRequest(work)) }
            return .done
        case .retry:
            guard let current else { return .done }
            await send(work.workKey) { try await self.service.retry(id: current.id) }
            return .done
        }
    }

    func open(_ work: CatalogWork) {
        sheet?.stop()
        let workKey = work.workKey
        sheet = CatalogSheetModel(
            work: work,
            request: shown(work),
            service: service,
            pollNanos: pollNanos,
            onChange: { [weak self] next in
                self?.writeOverlay(workKey, next)
            },
            onImported: onImported
        )
    }

    func close() {
        let current = sheet
        sheet = nil
        current?.stop()
    }

    private func writeOverlay(_ workKey: String, _ request: BookRequest?) {
        overlay[workKey] = .some(request)
    }

    private func send(_ workKey: String, _ block: () async throws -> BookRequest) async {
        notice = nil
        do {
            let next = try await block()
            writeOverlay(workKey, next)
            if next.state == "imported" { onImported() }
        } catch is CancellationError {
            return
        } catch {
            notice = catalogError(from: error).message
        }
    }

    private func start(append: Bool, debounce: Bool, quietMemo: Bool) {
        loadTask?.cancel()
        generation += 1
        let gen = generation
        let mode = catalogQuery(query)
        let token = service.token
        guard service.access == .on, let token else {
            if !append { phase = .idle }
            loadTask = nil
            return
        }

        let pageNumber: Int
        var quiet = false
        if append {
            guard case .loaded(let page, false) = phase, let next = page.nextPage else { return }
            phase = .loaded(page, more: true)
            pageNumber = next
        } else if quietMemo, case .browse = mode, let memo = trending[token] {
            phase = .loaded(memo.page, more: false)
            if memo.pages > 1 {
                loadTask = nil
                return
            }
            quiet = true
            pageNumber = 1
        } else {
            let previous: CatalogPage?
            if case .search = mode {
                previous = shownPage
            } else {
                previous = nil
            }
            phase = .loading(previous)
            pageNumber = 1
        }

        let shouldDebounce = debounce && !append && !quiet && isSearch(mode)
        let queryParam = searchText(mode)
        loadTask = Task { [weak self] in
            guard let self else { return }
            if shouldDebounce {
                do {
                    try await Task.sleep(nanoseconds: self.debounceNanos)
                } catch {
                    return
                }
            }
            guard !Task.isCancelled, gen == self.generation, self.service.token == token else { return }
            do {
                let page = try await self.service.page(query: queryParam, page: pageNumber)
                guard !Task.isCancelled, gen == self.generation, self.service.token == token else { return }
                self.deliver(page, mode: mode, token: token, append: append)
            } catch is CancellationError {
                return
            } catch let url as URLError where url.code == .cancelled {
                return
            } catch {
                guard !Task.isCancelled, gen == self.generation, self.service.token == token else { return }
                if quiet, self.shownPage != nil { return }
                self.phase = .failed(catalogError(from: error).message)
            }
        }
    }

    private func deliver(_ page: CatalogPage, mode: CatalogQuery, token: String, append: Bool) {
        let merged: CatalogPage
        let pages: Int
        if append, let current = shownPage {
            merged = CatalogPage(items: current.items + page.items, nextPage: page.nextPage)
            pages = (trending[token]?.pages ?? 1) + 1
        } else {
            merged = page
            pages = 1
        }
        phase = .loaded(merged, more: false)
        if case .browse = mode {
            trending[token] = TrendingMemo(page: merged, pages: pages)
        }
    }

    private var shownPage: CatalogPage? {
        switch phase {
        case .loaded(let page, _): return page
        case .loading(let page): return page
        case .idle, .failed: return nil
        }
    }

    private func isSearch(_ mode: CatalogQuery) -> Bool {
        if case .search = mode { return true }
        return false
    }

    private func searchText(_ mode: CatalogQuery) -> String? {
        if case .search(let text) = mode { return text }
        return nil
    }
}

private struct TrendingMemo: Equatable, Sendable {
    var page: CatalogPage
    var pages: Int
}
