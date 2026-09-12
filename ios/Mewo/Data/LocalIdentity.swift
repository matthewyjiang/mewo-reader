import Foundation

struct LocalProfile: Equatable, Sendable {
    var name: String = ""
    var handle: String = ""

    var ready: Bool { !handle.isEmpty }

    var displayName: String {
        if !name.isEmpty { return name }
        if !handle.isEmpty { return handle }
        return "You"
    }
}

enum LocalIdentity {
    static let handleMin = 3
    static let handleMax = 32
    static let nameMax = 50

    static func normalizeHandle(_ raw: String) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.hasPrefix("@") ? String(trimmed.dropFirst()) : trimmed
    }

    static func nameError(_ raw: String) -> String? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.count <= nameMax { return nil }
        return "Name is longer than \(nameMax) characters (asked \(trimmed.count))."
    }

    static func handleError(_ raw: String) -> String? {
        let normalized = normalizeHandle(raw)
        if normalized.isEmpty { return nil }
        return parse(name: "", handle: raw).error
    }

    struct ParseResult {
        var profile: LocalProfile?
        var error: String?

        var isSuccess: Bool { profile != nil }
    }

    static func parse(name: String, handle: String) -> ParseResult {
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        if let err = nameError(trimmedName) {
            return ParseResult(error: err)
        }
        let normalized = normalizeHandle(handle)
        if normalized.isEmpty {
            return ParseResult(error: "Pick a handle.")
        }
        if normalized.count < handleMin || normalized.count > handleMax {
            return ParseResult(
                error: "Handle must be \(handleMin) to \(handleMax) characters (asked \(normalized.count))."
            )
        }
        if !normalized.unicodeScalars.allSatisfy({ scalar in
            CharacterSet.alphanumerics.contains(scalar) || scalar == "_"
        }) || normalized.contains(where: { $0.isLetter && !$0.isASCII }) {
            // Restrict to ASCII letters, digits, underscore.
        }
        if !normalized.allSatisfy({ ch in
            ch.isASCII && (ch.isLetter || ch.isNumber || ch == "_")
        }) {
            return ParseResult(error: "Handle can only use letters, digits, and underscore.")
        }
        return ParseResult(profile: LocalProfile(name: trimmedName, handle: normalized))
    }
}
