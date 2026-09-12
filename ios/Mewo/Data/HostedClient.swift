import Foundation

struct HostedException: LocalizedError {
    var message: String
    var code: Int = 0
    var errorDescription: String? { message }
}

final class HostedClient: Sendable {
    private let session: URLSession

    init() {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 120
        config.timeoutIntervalForResource = 180
        session = URLSession(configuration: config)
    }

    func register(baseUrl: String, username: String, password: String) async throws -> AuthResponse {
        try await postAuth("\(baseUrl)/v1/auth/register", username: username, password: password)
    }

    func login(baseUrl: String, username: String, password: String) async throws -> AuthResponse {
        try await postAuth("\(baseUrl)/v1/auth/login", username: username, password: password)
    }

    func logout(_ hosted: HostedSession) async throws {
        var request = try authorized(hosted, path: "/v1/auth/logout")
        request.httpMethod = "POST"
        request.httpBody = Data()
        _ = try await expectOk(request)
    }

    func library(_ hosted: HostedSession) async throws -> LibrarySnapshot {
        try await get(hosted, path: "/v1/library")
    }

    func upload(
        _ hosted: HostedSession,
        epub: URL,
        cover: URL?,
        title: String,
        author: String,
        handle: String,
        isSample: Bool
    ) async throws -> BookRecord {
        let boundary = "mewo-\(UUID().uuidString)"
        var request = try authorized(hosted, path: "/v1/books")
        request.httpMethod = "POST"
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        var body = Data()
        func field(_ name: String, _ value: String) {
            body.append("--\(boundary)\r\n".data(using: .utf8)!)
            body.append("Content-Disposition: form-data; name=\"\(name)\"\r\n\r\n".data(using: .utf8)!)
            body.append("\(value)\r\n".data(using: .utf8)!)
        }
        field("title", title)
        field("author", author)
        field("handle", handle)
        field("isSample", isSample ? "true" : "false")
        try appendFile(&body, boundary: boundary, name: "epub", filename: "book.epub", mime: "application/epub+zip", url: epub)
        if let cover, FileManager.default.fileExists(atPath: cover.path),
           (try? cover.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0 > 0 {
            try appendFile(&body, boundary: boundary, name: "cover", filename: "cover.jpg", mime: "image/jpeg", url: cover)
        }
        body.append("--\(boundary)--\r\n".data(using: .utf8)!)
        request.httpBody = body
        return try await decode(request)
    }

    func delete(_ hosted: HostedSession, id: String) async throws {
        var request = try authorized(hosted, path: "/v1/books/\(id)")
        request.httpMethod = "DELETE"
        _ = try await expectOk(request)
    }

    func patch(_ hosted: HostedSession, id: String, body: PatchBook) async throws -> BookRecord {
        var request = try authorized(hosted, path: "/v1/books/\(id)")
        request.httpMethod = "PATCH"
        request.setValue("application/json; charset=utf-8", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONFiles.encoder.encode(body)
        return try await decode(request)
    }

    func downloadEpub(_ hosted: HostedSession, id: String, dest: URL) async throws {
        try await download(hosted, path: "/v1/books/\(id)/epub", dest: dest)
    }

    func downloadCover(_ hosted: HostedSession, id: String, dest: URL) async throws -> Bool {
        do {
            try await download(hosted, path: "/v1/books/\(id)/cover", dest: dest)
            return FileManager.default.fileExists(atPath: dest.path)
        } catch let err as HostedException where err.code == 401 {
            throw err
        } catch {
            return false
        }
    }

    func likes(_ hosted: HostedSession, id: String) async throws -> Set<String> {
        let set: LikeSet = try await get(hosted, path: "/v1/books/\(id)/likes")
        return set.ids
    }

    func toggleLike(_ hosted: HostedSession, id: String, postId: String) async throws -> Set<String> {
        var request = try authorized(hosted, path: "/v1/books/\(id)/likes/\(postId)")
        request.httpMethod = "POST"
        request.httpBody = Data()
        let set: LikeSet = try await decode(request)
        return set.ids
    }

    func commentIndex(_ hosted: HostedSession, id: String) async throws -> CommentIndex {
        try await get(hosted, path: "/v1/books/\(id)/comments")
    }

    func comments(_ hosted: HostedSession, id: String, postId: String) async throws -> [PostComment] {
        let thread: CommentThread = try await get(hosted, path: "/v1/books/\(id)/comments/\(postId)")
        return thread.comments
    }

    func addComment(_ hosted: HostedSession, id: String, postId: String, text: String) async throws -> [PostComment] {
        var request = try authorized(hosted, path: "/v1/books/\(id)/comments/\(postId)")
        request.httpMethod = "POST"
        request.setValue("application/json; charset=utf-8", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONFiles.encoder.encode(NewComment(text: text))
        let thread: CommentThread = try await decode(request)
        return thread.comments
    }

    func deleteComment(_ hosted: HostedSession, id: String, postId: String, commentId: String) async throws -> [PostComment] {
        var request = try authorized(hosted, path: "/v1/books/\(id)/comments/\(postId)/\(commentId)")
        request.httpMethod = "DELETE"
        let thread: CommentThread = try await decode(request)
        return thread.comments
    }

    func profileReplies(_ hosted: HostedSession, handle: String) async throws -> ProfileReplies {
        let encoded = handle.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? handle
        return try await get(hosted, path: "/v1/profiles/\(encoded)/replies")
    }

    private func postAuth(_ url: String, username: String, password: String) async throws -> AuthResponse {
        guard let target = URL(string: url) else {
            throw HostedException(message: "Set a server URL first.")
        }
        var request = URLRequest(url: target)
        request.httpMethod = "POST"
        request.setValue("application/json; charset=utf-8", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONFiles.encoder.encode(AuthRequest(username: username, password: password))
        return try await decode(request)
    }

    private func get<T: Decodable>(_ hosted: HostedSession, path: String) async throws -> T {
        let request = try authorized(hosted, path: path)
        return try await decode(request)
    }

    private func download(_ hosted: HostedSession, path: String, dest: URL) async throws {
        let request = try authorized(hosted, path: path)
        let (data, response) = try await session.data(for: request)
        try expectOk(response, data: data)
        try FileManager.default.createDirectory(at: dest.deletingLastPathComponent(), withIntermediateDirectories: true)
        let tmp = dest.appendingPathExtension("part")
        try data.write(to: tmp, options: .atomic)
        if FileManager.default.fileExists(atPath: dest.path) {
            try FileManager.default.removeItem(at: dest)
        }
        try FileManager.default.moveItem(at: tmp, to: dest)
    }

    private func authorized(_ hosted: HostedSession, path: String) throws -> URLRequest {
        guard let base = hosted.normalizedUrl(), let url = URL(string: base + path) else {
            throw HostedException(message: "Set a server URL first.")
        }
        var request = URLRequest(url: url)
        if let token = hosted.token, !token.isEmpty {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        return request
    }

    private func decode<T: Decodable>(_ request: URLRequest) async throws -> T {
        let data = try await expectOk(request)
        do {
            return try JSONFiles.decoder.decode(T.self, from: data)
        } catch {
            throw HostedException(message: "Could not read the server response.")
        }
    }

    private func expectOk(_ request: URLRequest) async throws -> Data {
        let (data, response) = try await session.data(for: request)
        try expectOk(response, data: data)
        return data
    }

    private func expectOk(_ response: URLResponse, data: Data) throws {
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        if (200..<300).contains(code) { return }
        let message = (try? JSONFiles.decoder.decode(HostedErrorBody.self, from: data).error)
            .flatMap { $0.isEmpty ? nil : $0 }
            ?? "Server returned \(code)"
        throw HostedException(message: message, code: code)
    }

    private func appendFile(_ body: inout Data, boundary: String, name: String, filename: String, mime: String, url: URL) throws {
        body.append("--\(boundary)\r\n".data(using: .utf8)!)
        body.append("Content-Disposition: form-data; name=\"\(name)\"; filename=\"\(filename)\"\r\n".data(using: .utf8)!)
        body.append("Content-Type: \(mime)\r\n\r\n".data(using: .utf8)!)
        body.append(try Data(contentsOf: url))
        body.append("\r\n".data(using: .utf8)!)
    }
}
