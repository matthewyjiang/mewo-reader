import Foundation

/// Sheet label for a request. A server `phase` wins. Without one, the list row
/// uses the same rule as the server's `phase`.
enum RequestPhase: String, Equatable, Sendable {
    case searching
    case waiting
    case noMatch = "no_match"
    case held
    case retrying
    case downloading
    case importing
    case failed
    case canceled
    case imported

    var isLive: Bool {
        switch self {
        case .searching, .waiting, .noMatch, .held, .retrying, .downloading, .importing:
            return true
        case .failed, .canceled, .imported:
            return false
        }
    }
}

let catalogMinScalars = 3
let catalogDebounceNanos: UInt64 = 300_000_000
let catalogPollNanos: UInt64 = 10_000_000_000

enum CatalogQuery: Equatable, Sendable {
    case browse
    case search(String)
}

/// Blank, or fewer than 3 Unicode scalars, stays on trending.
func catalogQuery(_ raw: String) -> CatalogQuery {
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.unicodeScalars.count < catalogMinScalars { return .browse }
    return .search(trimmed)
}

/// Browse, including a 1–2 character field, is still the trending list.
func catalogHeading(_ query: String) -> String? {
    if case .browse = catalogQuery(query) { return "Trending on Open Library" }
    return nil
}

func catalogEmptyCopy(_ query: String) -> String {
    switch catalogQuery(query) {
    case .browse:
        return "No trending books in your languages right now. Search above."
    case .search(let text):
        return "Nothing in Open Library matches “\(text)”. Try the author's name or fewer words."
    }
}

func isLivePhase(_ phase: RequestPhase?) -> Bool {
    phase?.isLive ?? false
}

private let holdServices = ["SABnzbd", "Prowlarr", "Open Library"]

func holdService(_ error: String?) -> String? {
    guard let error else { return nil }
    return holdServices.first { error.hasPrefix("\($0):") }
}

func closestTitle(_ error: String?) -> String? {
    guard let error else { return nil }
    let pattern = #"\(closest: (.*?), a different book\)"#
    guard let regex = try? NSRegularExpression(pattern: pattern) else { return nil }
    let range = NSRange(error.startIndex..., in: error)
    guard let match = regex.firstMatch(in: error, range: range),
          match.numberOfRanges > 1,
          let title = Range(match.range(at: 1), in: error) else { return nil }
    return String(error[title])
}

func requestPhase(_ request: BookRequest, phase: String? = nil) -> RequestPhase {
    if let phase, let parsed = RequestPhase(rawValue: phase) { return parsed }
    let error = request.error ?? ""
    if request.state == "imported" { return .imported }
    if request.state == "failed" { return .failed }
    if request.state == "canceled" { return .canceled }
    if request.state == "downloading" {
        if holdService(error) != nil { return .held }
        if request.progress >= 100 { return .importing }
        return .downloading
    }
    if request.state == "requested" {
        if error == "No release yet." { return .waiting }
        if error.hasPrefix("No matching release") { return .noMatch }
        if holdService(error) != nil { return .held }
        if !error.isEmpty { return .retrying }
    }
    return .searching
}

func requestPhase(_ detail: RequestDetail) -> RequestPhase {
    requestPhase(detail.request, phase: detail.phase)
}

func stateLabel(_ request: BookRequest, phase explicit: String? = nil) -> String {
    let phase = requestPhase(request, phase: explicit)
    switch phase {
    case .searching: return "Searching"
    case .waiting: return "Waiting for a release"
    case .noMatch: return "No matching release"
    case .held:
        if let service = holdService(request.error) { return "Waiting on \(service)" }
        return "Waiting"
    case .retrying: return "Retrying"
    case .downloading: return "Downloading \(request.progress)%"
    case .importing: return "Importing"
    case .imported: return "On your shelf"
    case .failed: return "Failed: \(request.error ?? "")"
    case .canceled: return "Canceled"
    }
}

enum RowAction: String, Equatable, Sendable {
    case request, pending, open, retry, again
}

/// Retry is only legal for a failed row. A canceled row uses the same plus as
/// "Request again" and still calls POST /retry. An imported row with no book,
/// and no copy already on the server, starts over with POST /v1/requests.
func rowAction(_ request: BookRequest?, _ onServer: ServerCopy? = nil) -> RowAction {
    if let request, request.state == "requested" || request.state == "downloading" { return .pending }
    if request?.state == "failed" { return .retry }
    if request?.state == "imported", request?.bookId != nil { return .open }
    if onServer?.bookId != nil { return .open }
    if request?.state == "canceled" { return .again }
    if request?.state == "imported" { return .again }
    return .request
}

func rowActionLabel(_ action: RowAction, _ work: CatalogWork, _ request: BookRequest?) -> String {
    switch action {
    case .request: return "Request"
    case .again: return "Request again"
    case .pending: return request.map { stateLabel($0) } ?? "Request"
    case .open: return openLabel(work, request)
    case .retry: return "Retry"
    }
}

/// A live row already says its status. No trailing control, so a clock does not
/// sit beside that line looking like a button.
func rowSymbol(_ action: RowAction) -> String? {
    switch action {
    case .request, .again: return "plus"
    case .open: return "book"
    case .retry: return "arrow.clockwise"
    case .pending: return nil
    }
}

func openLabel(_ work: CatalogWork, _ request: BookRequest?) -> String {
    if request?.state == "imported" || request?.bookId != nil { return "On your shelf" }
    let copy = work.onServer
    if copy == nil || copy?.mine == true || copy?.held == true { return "On your shelf" }
    return "On server"
}

enum CopyLabelStyle: Equatable, Sendable {
    case badge, sheet
}

/// A held copy never names its owner, even when `ownerUsername` is filled in.
func copyLabel(_ copy: ServerCopy, _ style: CopyLabelStyle) -> String {
    if copy.mine || copy.held { return "On your shelf" }
    if copy.shared, !copy.ownerUsername.isEmpty {
        return style == .badge
            ? "On server · @\(copy.ownerUsername)"
            : "On server, shared by @\(copy.ownerUsername)"
    }
    return "On server"
}

func badge(_ work: CatalogWork, _ request: BookRequest?, phase: String? = nil) -> String? {
    if let request, request.state == "requested" || request.state == "downloading" || request.state == "failed" {
        return stateLabel(request, phase: phase)
    }
    if request?.state == "imported", request?.bookId != nil { return "On your shelf" }
    if let copy = work.onServer { return copyLabel(copy, .badge) }
    return nil
}

enum BadgeTone: Equatable, Sendable {
    case shelf, failed, server, progress
}

func badgeTone(_ label: String) -> BadgeTone {
    if label.hasPrefix("Failed") { return .failed }
    if label == "On your shelf" { return .shelf }
    if label.hasPrefix("On server") { return .server }
    return .progress
}

func catalogRowLabel(_ work: CatalogWork, _ request: BookRequest?) -> String {
    var parts = [work.title]
    if !work.author.isEmpty { parts.append(work.author) }
    if let year = work.year { parts.append(String(year)) }
    if let status = badge(work, request) { parts.append(status) }
    return parts.joined(separator: ", ")
}

func catalogOlid(_ workKey: String) -> String {
    guard let slash = workKey.lastIndex(of: "/") else { return workKey }
    return String(workKey[workKey.index(after: slash)...])
}

func formatCheck(_ at: Int64) -> String {
    let formatter = DateFormatter()
    formatter.locale = .current
    formatter.dateFormat = "MMM d, h:mm a"
    return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(at) / 1000))
}

func waitingCopy(_ nextCheckAt: Int64?) -> String {
    guard let nextCheckAt else { return "No release yet." }
    return "No release yet. Next search \(formatCheck(nextCheckAt))"
}

func noMatchCopy(_ error: String?, _ nextCheckAt: Int64?) -> String {
    let found: String
    if let closest = closestTitle(error) {
        found = "Only found a different book (\(closest))."
    } else {
        found = "Only found a different book."
    }
    guard let nextCheckAt else { return found }
    return "\(found) Searching again \(formatCheck(nextCheckAt))."
}

func heldCopy(_ error: String?) -> String {
    guard let service = holdService(error) else {
        if let error, !error.isEmpty { return "Waiting: \(error)" }
        return "Waiting on the download service."
    }
    let rest = error.map { String($0.dropFirst(service.count + 1)).trimmingCharacters(in: .whitespacesAndNewlines) } ?? ""
    return rest.isEmpty ? "Waiting on \(service)" : "Waiting on \(service): \(rest)"
}

func alsoRequested(_ names: [String]) -> String {
    let shown = names.prefix(3).map { "@\($0)" }
    let extra = names.count - shown.count
    let list = shown.joined(separator: ", ")
    return extra > 0 ? "Also requested by \(list) +\(extra)" : "Also requested by \(list)"
}

func workStats(_ info: WorkInfo) -> String {
    var parts: [String] = []
    if let year = info.year { parts.append("First published \(year)") }
    if let pages = info.pagesMedian { parts.append("~\(groupedNumber(pages)) pages") }
    if let rating = info.rating {
        parts.append("★ \(String(format: "%.1f", rating.average))")
    }
    return parts.joined(separator: " · ")
}

func subjectLine(_ subjects: [String]) -> String {
    subjects.prefix(5).joined(separator: " · ")
}

func coverUrl(_ coverId: Int?) -> String? {
    guard let coverId, coverId >= 0 else { return nil }
    return "https://covers.openlibrary.org/b/id/\(coverId)-M.jpg?default=false"
}

func capNote(_ status: WorkStatus) -> String? {
    guard status.active >= status.cap else { return nil }
    return "You have \(status.cap) requests in progress"
}

enum SheetStatus: Equatable, Sendable {
    case live(phase: RequestPhase, line: String, progress: Int?)
    case failed(line: String, capNote: String?)
    case available(line: String, bookId: String?)
    case canceled(capNote: String?)
    case requestAgain(capNote: String?)
    case requestable(capNote: String?, editionNote: String?)
}

func sheetStatus(
    phase: RequestPhase?,
    request: BookRequest?,
    detailRequest: RequestDetail?,
    copy: ServerCopy?,
    bookId: String?,
    capNote: String?,
    hasAcceptedEdition: Bool?
) -> SheetStatus {
    if let phase, phase.isLive {
        switch phase {
        case .searching:
            return .live(phase: phase, line: "Looking for a release…", progress: nil)
        case .waiting:
            return .live(phase: phase, line: waitingCopy(detailRequest?.nextCheckAt), progress: nil)
        case .noMatch:
            return .live(phase: phase, line: noMatchCopy(request?.error, detailRequest?.nextCheckAt), progress: nil)
        case .held:
            return .live(phase: phase, line: heldCopy(request?.error), progress: nil)
        case .downloading:
            let value = request?.progress ?? 0
            return .live(phase: phase, line: "Downloading \(value)%", progress: value)
        case .retrying:
            let error = request?.error ?? ""
            let line = error.isEmpty ? "Trying another release…" : error
            return .live(phase: phase, line: line, progress: nil)
        case .importing:
            return .live(phase: phase, line: "Checking and importing…", progress: request?.progress ?? 0)
        case .failed, .canceled, .imported:
            break
        }
    }
    if phase == .failed, let request {
        return .failed(line: request.error ?? "The download failed.", capNote: capNote)
    }
    // An import that never received a book, and nothing already on the server,
    // is not on the shelf. "Open book" would sit there disabled.
    if phase == .imported, bookId == nil, copy == nil {
        return .requestAgain(capNote: capNote)
    }
    if copy != nil || phase == .imported {
        let line = copy.map { copyLabel($0, .sheet) } ?? "On your shelf"
        return .available(line: line, bookId: bookId)
    }
    if phase == .canceled {
        return .canceled(capNote: capNote)
    }
    let edition = hasAcceptedEdition == false ? "No English edition listed on Open Library" : nil
    return .requestable(capNote: capNote, editionNote: edition)
}

func catalogErrorText(code: Int?, serverMessage: String?, network: Bool) -> String {
    if network {
        return "Couldn't reach the server. Check your connection."
    }
    switch code {
    case 401:
        return "Sign in again."
    case 403:
        return "Requests are off."
    case 404:
        return "That book is no longer in the catalog."
    default:
        break
    }
    if let serverMessage {
        let trimmed = serverMessage.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty, !isRawStatus(trimmed) { return trimmed }
    }
    return "Couldn't load the catalog."
}

func catalogError(from error: Error) -> CatalogError {
    if let catalog = error as? CatalogError { return catalog }
    if let hosted = error as? HostedException {
        let reason: CatalogErrorReason
        let network: Bool
        switch hosted.code {
        case 401:
            reason = .unauthorized
            network = false
        case 403:
            reason = .forbidden
            network = false
        case 404:
            reason = .notFound
            network = false
        case 0:
            let lower = hosted.message.lowercased()
            if lower.contains("connection") || lower.contains("reach") || lower.contains("network") || lower.contains("offline") {
                reason = .network
                network = true
            } else {
                reason = .unreadable
                network = false
            }
        default:
            reason = .http(hosted.code)
            network = false
        }
        let code = hosted.code == 0 ? nil : hosted.code
        return CatalogError(
            reason: reason,
            message: catalogErrorText(code: code, serverMessage: hosted.message, network: network)
        )
    }
    if error is URLError {
        return CatalogError(reason: .network, message: catalogErrorText(code: nil, serverMessage: nil, network: true))
    }
    return CatalogError(reason: .unreadable, message: catalogErrorText(code: nil, serverMessage: nil, network: false))
}

private func isRawStatus(_ message: String) -> Bool {
    message.range(of: #"(?i)^(server returned \d+|http \d+)\b"#, options: .regularExpression) != nil
}

private func groupedNumber(_ value: Int) -> String {
    let formatter = NumberFormatter()
    formatter.numberStyle = .decimal
    return formatter.string(from: NSNumber(value: value)) ?? String(value)
}
