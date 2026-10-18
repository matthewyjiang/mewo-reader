import CryptoKit
import Foundation

/// One shelf file per server URL and username.
func shelfCacheKey(serverUrl: String, username: String) -> String {
    let material = normalizeBaseUrl(serverUrl) + "\n" + username
    let digest = SHA256.hash(data: Data(material.utf8))
    return digest.map { String(format: "%02x", $0) }.joined()
}

struct ShelfCache: Sendable {
    var directory: URL

    func read(serverUrl: String, username: String) -> [BookRecord]? {
        if serverUrl.isEmpty || username.isEmpty { return nil }
        return JSONFiles.read(LibrarySnapshot.self, from: file(serverUrl, username))?.books
    }

    func write(serverUrl: String, username: String, books: [BookRecord]) throws {
        if serverUrl.isEmpty || username.isEmpty { return }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let dest = file(serverUrl, username)
        let tmp = directory.appendingPathComponent(dest.lastPathComponent + ".tmp")
        try JSONFiles.write(LibrarySnapshot(books: books), to: tmp)
        if FileManager.default.fileExists(atPath: dest.path) {
            try FileManager.default.removeItem(at: dest)
        }
        try FileManager.default.moveItem(at: tmp, to: dest)
    }

    func clear(serverUrl: String, username: String) {
        if serverUrl.isEmpty || username.isEmpty { return }
        try? FileManager.default.removeItem(at: file(serverUrl, username))
    }

    private func file(_ serverUrl: String, _ username: String) -> URL {
        directory.appendingPathComponent(shelfCacheKey(serverUrl: serverUrl, username: username) + ".json")
    }
}
