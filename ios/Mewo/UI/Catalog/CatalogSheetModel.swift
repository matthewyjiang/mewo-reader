import Foundation
import Observation
import UIKit

struct CatalogAbout: Equatable, Sendable {
    var blurb: String?
    var series: [String]
    var awards: [String]
    var subjects: String
    var stats: String
    var libraryURL: URL?
}

enum CatalogAboutState: Equatable, Sendable {
    case loading
    case failed(String)
    case missing
    case ready(CatalogAbout)
}

@Observable
@MainActor
final class CatalogSheetModel: Identifiable {
    let work: CatalogWork
    var id: String { work.workKey }
    private(set) var detail: WorkDetail?
    private(set) var loading = true
    private(set) var loadError: String?
    private(set) var acting = false
    var notice: String?
    var expanded = false
    private var blurbWidth: CGFloat = 0
    private var blurbContentSize: UIContentSizeCategory = .large

    @ObservationIgnored private var seed: BookRequest?
    @ObservationIgnored private var importedShelf = false
    @ObservationIgnored private var stopped = false
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var pollTask: Task<Void, Never>?
    @ObservationIgnored private let service: CatalogService
    @ObservationIgnored private let pollNanos: UInt64
    @ObservationIgnored private let onChange: (BookRequest?) -> Void
    @ObservationIgnored private let onImported: () -> Void

    init(
        work: CatalogWork,
        request: BookRequest?,
        service: CatalogService,
        pollNanos: UInt64 = catalogPollNanos,
        onChange: @escaping (BookRequest?) -> Void,
        onImported: @escaping () -> Void
    ) {
        self.work = work
        self.seed = request
        self.service = service
        self.pollNanos = pollNanos
        self.onChange = onChange
        self.onImported = onImported
        beginPolling()
    }

    var atCap: Bool {
        guard let status = detail?.status else { return false }
        return status.active >= status.cap
    }

    var status: SheetStatus {
        let live = liveRequest
        let phase = livePhase
        let detailRequest = detail?.status.request
        let note: String?
        if phase?.isLive == true {
            note = nil
        } else {
            note = detail.flatMap { capNote($0.status) }
        }
        return sheetStatus(
            phase: phase,
            request: live,
            detailRequest: detailRequest,
            copy: detail?.status.onServer ?? work.onServer,
            bookId: bookId,
            capNote: note,
            hasAcceptedEdition: detail?.info?.hasAcceptedEdition
        )
    }

    /// Names from `status.otherRequests` only. A held copy's owner stays out.
    var alsoRequested: String? {
        let names = detail?.status.otherRequests.map(\.requester) ?? []
        guard !names.isEmpty else { return nil }
        return requestedBy(names)
    }

    var about: CatalogAboutState {
        if let loadError, detail?.info == nil { return .failed(loadError) }
        if loading, detail?.info == nil { return .loading }
        guard let info = detail?.info else { return .missing }
        return .ready(CatalogAbout(
            blurb: info.description.flatMap { $0.isEmpty ? nil : $0 },
            series: info.series,
            awards: info.awards,
            subjects: subjectLine(info.subjects),
            stats: workStats(info),
            libraryURL: openLibraryURL(info.canonicalKey)
        ))
    }

    /// The laid-out blurb is taller than the 6-line clamp. A character count
    /// misses a short column and a wide one.
    var blurbOverflow: Bool {
        guard case .ready(let about) = self.about, let blurb = about.blurb else { return false }
        return Self.blurbOverflow(text: blurb, width: blurbWidth, contentSize: blurbContentSize)
    }

    func noteBlurbLayout(width: CGFloat, contentSize: UIContentSizeCategory) {
        guard width > 0 else { return }
        guard blurbWidth != width || blurbContentSize != contentSize else { return }
        blurbWidth = width
        blurbContentSize = contentSize
    }

    /// `MewoFont.regular(17)` is a body-relative custom font, so the measurement
    /// has to grow with the same text style. A fixed 17pt font hides More once
    /// Dynamic Type wraps the blurb past six lines.
    static func blurbFont(contentSize: UIContentSizeCategory = .large) -> UIFont {
        let base = UIFont(name: MewoFont.regular, size: 17) ?? .systemFont(ofSize: 17)
        let traits = UITraitCollection(preferredContentSizeCategory: contentSize)
        return UIFontMetrics(forTextStyle: .body).scaledFont(for: base, compatibleWith: traits)
    }

    /// Unclamped height against six lines of the same font at `width`.
    static func blurbOverflow(
        text: String,
        width: CGFloat,
        contentSize: UIContentSizeCategory = .large
    ) -> Bool {
        guard width > 0 else { return false }
        let font = blurbFont(contentSize: contentSize)
        let full = (text as NSString).boundingRect(
            with: CGSize(width: width, height: .greatestFiniteMagnitude),
            options: [.usesLineFragmentOrigin, .usesFontLeading],
            attributes: [.font: font],
            context: nil
        ).height
        return full > font.lineHeight * 6 + 1
    }

    func reload() {
        guard !stopped else { return }
        loadError = nil
        if detail == nil { loading = true }
        beginPolling()
    }

    func request() async {
        guard !acting, !atCap, !stopped else { return }
        acting = true
        defer { acting = false }
        notice = nil
        pausePoll()
        do {
            let next = try await service.request(NewBookRequest(work))
            guard !stopped else { return }
            seed = next
            onChange(next)
            if next.state == "imported" {
                importedShelf = true
                onImported()
            }
        } catch is CancellationError {
            return
        } catch {
            notice = catalogError(from: error).message
        }
        beginPolling()
    }

    func cancel() async {
        let current = liveRequest
        guard !acting, !stopped, let current else { return }
        acting = true
        defer { acting = false }
        notice = nil
        pausePoll()
        let canceled = canceledRequest(current)
        seed = canceled
        applyOptimisticCancel(canceled)
        onChange(canceled)
        do {
            try await service.cancel(id: current.id)
        } catch is CancellationError {
            return
        } catch {
            notice = catalogError(from: error).message
        }
        beginPolling()
    }

    func retry() async {
        let current = liveRequest
        guard !acting, !atCap, !stopped, let current else { return }
        acting = true
        defer { acting = false }
        notice = nil
        pausePoll()
        do {
            let next = try await service.retry(id: current.id)
            guard !stopped else { return }
            seed = next
            onChange(next)
            if next.state == "imported" {
                importedShelf = true
                onImported()
            }
        } catch is CancellationError {
            return
        } catch {
            notice = catalogError(from: error).message
        }
        beginPolling()
    }

    func stop() {
        stopped = true
        generation += 1
        pollTask?.cancel()
        pollTask = nil
    }

    private var liveRequest: BookRequest? {
        detail?.status.request?.request ?? seed
    }

    private var livePhase: RequestPhase? {
        if let detail = detail?.status.request { return requestPhase(detail) }
        if let seed { return requestPhase(seed) }
        return nil
    }

    private var bookId: String? {
        detail?.status.request?.request.bookId
            ?? detail?.status.onServer?.bookId
            ?? seed?.bookId
            ?? work.onServer?.bookId
    }

    private func pausePoll() {
        generation += 1
        pollTask?.cancel()
        pollTask = nil
    }

    private func beginPolling() {
        guard !stopped else { return }
        generation += 1
        pollTask?.cancel()
        let gen = generation
        pollTask = Task { [weak self] in
            await self?.poll(gen)
        }
    }

    private func poll(_ gen: Int) async {
        while !Task.isCancelled && gen == generation && !stopped {
            let keep = await pull(gen)
            if !keep || gen != generation || Task.isCancelled || stopped { return }
            do {
                try await Task.sleep(nanoseconds: pollNanos)
            } catch {
                return
            }
        }
    }

    private func pull(_ gen: Int) async -> Bool {
        do {
            let next = try await service.work(workKey: work.workKey)
            guard gen == generation, !Task.isCancelled, !stopped else { return false }
            apply(next)
            return isLivePhase(next.status.request.map { requestPhase($0) })
        } catch is CancellationError {
            return false
        } catch let url as URLError where url.code == .cancelled {
            return false
        } catch {
            guard gen == generation, !Task.isCancelled, !stopped else { return false }
            let catalog = catalogError(from: error)
            if detail == nil {
                loadError = catalog.message
                loading = false
            }
            switch catalog.reason {
            case .unauthorized, .forbidden, .notFound:
                return false
            default:
                return true
            }
        }
    }

    private func apply(_ next: WorkDetail) {
        let opening = detail == nil
        let previous = detail?.status.request
        detail = next
        loadError = nil
        loading = false
        let request = next.status.request?.request
        if !sameRequest(previous, next.status.request) {
            seed = request
            onChange(request)
        }
        guard let phase = next.status.request.map({ requestPhase($0) }), phase == .imported else { return }
        if opening {
            importedShelf = true
            return
        }
        if !importedShelf {
            importedShelf = true
            onImported()
        }
    }

    private func applyOptimisticCancel(_ request: BookRequest) {
        guard var detail else { return }
        guard detail.status.request?.request.id == request.id else { return }
        let current = detail.status.request
        detail.status.request = RequestDetail(
            request: request,
            attempts: current?.attempts ?? 0,
            maxAttempts: current?.maxAttempts ?? 0,
            nextCheckAt: nil,
            releaseTitle: current?.releaseTitle,
            releaseSize: current?.releaseSize,
            phase: RequestPhase.canceled.rawValue
        )
        self.detail = detail
    }

    private func sameRequest(_ left: RequestDetail?, _ right: RequestDetail?) -> Bool {
        if left == nil && right == nil { return true }
        guard let left, let right else { return false }
        return left.request.id == right.request.id
            && left.request.state == right.request.state
            && left.request.progress == right.request.progress
            && left.request.error == right.request.error
            && left.request.bookId == right.request.bookId
            && left.phase == right.phase
    }

    private func canceledRequest(_ request: BookRequest) -> BookRequest {
        var copy = request
        copy.state = "canceled"
        copy.error = nil
        return copy
    }
}

private func openLibraryURL(_ canonicalKey: String) -> URL? {
    if canonicalKey.hasPrefix("http://") || canonicalKey.hasPrefix("https://") {
        return URL(string: canonicalKey)
    }
    let path = canonicalKey.hasPrefix("/") ? canonicalKey : "/\(canonicalKey)"
    return URL(string: "https://openlibrary.org\(path)")
}

/// The model property shares this name. A file-level wrapper keeps the call
/// on the free function.
private func requestedBy(_ names: [String]) -> String {
    alsoRequested(names)
}
