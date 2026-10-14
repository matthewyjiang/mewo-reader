import Foundation

/// Values shown on Settings → About.
///
/// The short version and build number come from the built Info.plist
/// (`CFBundleShortVersionString`, `CFBundleVersion`). The embed script writes
/// the git describe version over the short version and, when git is available,
/// adds `MewoGitCommit`.
enum AppBuild {
    static var displayLabel: String {
        format(
            version: bundleString("CFBundleShortVersionString") ?? "Unknown",
            build: bundleString("CFBundleVersion") ?? "",
            commit: bundleString("MewoGitCommit")
        )
    }

    /// `version (build · commit)`. The commit is omitted when the build did not embed one.
    static func format(version: String, build: String, commit: String?) -> String {
        let version = version.trimmingCharacters(in: .whitespacesAndNewlines)
        let shown = version.isEmpty ? "Unknown" : version
        let build = build.trimmingCharacters(in: .whitespacesAndNewlines)
        let commit = commit?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        var details: [String] = []
        if !build.isEmpty { details.append(build) }
        if !commit.isEmpty { details.append(commit) }
        guard !details.isEmpty else { return shown }
        return "\(shown) (\(details.joined(separator: " · ")))"
    }

    private static func bundleString(_ key: String) -> String? {
        guard let raw = Bundle.main.object(forInfoDictionaryKey: key) as? String else { return nil }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

/// Public `GET /v1/version` body: `{ "version": String, "commit": String? }`.
struct ServerVersion: Equatable, Sendable {
    var version: String
    var commit: String?

    var label: String {
        if let commit, !commit.isEmpty {
            return "\(version) (\(commit))"
        }
        return version
    }

    /// Nil when `version` is missing or not a non-empty string.
    /// A missing, null, empty, or non-string `commit` is treated as no commit.
    static func decode(from data: Data) -> ServerVersion? {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        guard let version = object["version"] as? String, !version.isEmpty else {
            return nil
        }
        let commit: String?
        if let raw = object["commit"] as? String, !raw.isEmpty {
            commit = raw
        } else {
            commit = nil
        }
        return ServerVersion(version: version, commit: commit)
    }
}
