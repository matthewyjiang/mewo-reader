import Foundation

struct HostedException: LocalizedError {
    var message: String
    var code: Int = 0
    var errorDescription: String? { message }
}

/// Tests replace these. The app leaves them nil.
final class HostedClientHooks: @unchecked Sendable {
    var library: (@Sendable (HostedSession) async throws -> LibrarySnapshot)?
    var posts: (@Sendable (HostedSession, String) async throws -> [FeedPost])?
    var postsProgress: (@Sendable (HostedSession, String, @escaping @Sendable (Double?) -> Void) async throws -> [FeedPost])?
    var likes: (@Sendable (HostedSession, String) async throws -> Set<String>)?
    var commentIndex: (@Sendable (HostedSession, String) async throws -> CommentIndex)?
    var comments: (@Sendable (HostedSession, String, String) async throws -> [PostComment])?
    var setLike: (@Sendable (HostedSession, String, String, Bool) async throws -> Set<String>)?
    var addComment: (@Sendable (HostedSession, String, String, String, String, Bool) async throws -> [PostComment])?
    var deleteComment: (@Sendable (HostedSession, String, String, String) async throws -> [PostComment])?
    var patch: (@Sendable (HostedSession, String, PatchBook) async throws -> BookRecord)?
    var downloadCover: (@Sendable (HostedSession, String, URL) async throws -> Bool)?
    var delete: (@Sendable (HostedSession, String) async throws -> Void)?
    var feed: (@Sendable (HostedSession, String?) async throws -> HomeFeedPage)?
}

final class HostedClient: Sendable {
    let hooks = HostedClientHooks()
    private let session: URLSession

    init(session: URLSession? = nil) {
        if let session {
            self.session = session
        } else {
            let config = URLSessionConfiguration.default
            config.timeoutIntervalForRequest = 120
            config.timeoutIntervalForResource = 180
            self.session = URLSession(configuration: config)
        }
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
        if let library = hooks.library { return try await library(hosted) }
        return try await get(hosted, path: "/v1/library")
    }

    /// Public version. No session token. `GET /v1/version`.
    func serverVersion(baseUrl: String) async throws -> ServerVersion {
        let base = normalizeBaseUrl(baseUrl)
        guard !base.isEmpty, let url = URL(string: base + "/v1/version") else {
            throw HostedException(message: "Set a server URL first.")
        }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.timeoutInterval = 15
        let data = try await expectOk(request)
        guard let version = ServerVersion.decode(from: data) else {
            throw HostedException(message: "Could not read the server response.")
        }
        return version
    }

    /// Public instance. No session token. `GET /v1/instance`.
    func instance(baseUrl: String) async throws -> InstanceInfo {
        let base = normalizeBaseUrl(baseUrl)
        guard !base.isEmpty, let url = URL(string: base + "/v1/instance") else {
            throw HostedException(message: "Set a server URL first.")
        }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.timeoutInterval = 15
        return try await decode(request)
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
        if let delete = hooks.delete { return try await delete(hosted, id) }
        var request = try authorized(hosted, path: "/v1/books/\(id)")
        request.httpMethod = "DELETE"
        _ = try await expectOk(request)
    }

    func patch(_ hosted: HostedSession, id: String, body: PatchBook) async throws -> BookRecord {
        if let patch = hooks.patch { return try await patch(hosted, id, body) }
        var request = try authorized(hosted, path: "/v1/books/\(id)")
        request.httpMethod = "PATCH"
        request.setValue("application/json; charset=utf-8", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONFiles.encoder.encode(body)
        return try await decode(request)
    }

    /// Timeline posts the server parsed from this book. Ids match likes and comments.
    /// `onProgress` is nil until the response has a Content-Length, then the fraction read.
    /// The server gzips this route, so the length is usually missing. That stays indeterminate.
    func posts(
        _ hosted: HostedSession,
        id: String,
        onProgress: @escaping @Sendable (Double?) -> Void = { _ in }
    ) async throws -> [FeedPost] {
        if let postsProgress = hooks.postsProgress {
            return try await postsProgress(hosted, id, onProgress)
        }
        if let posts = hooks.posts { return try await posts(hosted, id) }
        let request = try authorized(hosted, path: "/v1/books/\(id)/posts")
        let (data, response) = try await readBody(request, onProgress: onProgress)
        try expectOk(response, data: data)
        do {
            return try JSONFiles.decoder.decode(PostsResponse.self, from: data).posts
        } catch {
            throw HostedException(message: "Could not read the server response.", code: response.statusCode)
        }
    }

    func downloadCover(_ hosted: HostedSession, id: String, dest: URL) async throws -> Bool {
        if let downloadCover = hooks.downloadCover { return try await downloadCover(hosted, id, dest) }
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
        if let likes = hooks.likes { return try await likes(hosted, id) }
        let set: LikeSet = try await get(hosted, path: "/v1/books/\(id)/likes")
        return set.ids
    }

    /// Idempotent like. PUT sets it, DELETE clears it. The server's toggle route stays for old builds.
    func setLike(_ hosted: HostedSession, id: String, postId: String, on: Bool) async throws -> Set<String> {
        if let setLike = hooks.setLike { return try await setLike(hosted, id, postId, on) }
        var request = try authorized(hosted, path: "/v1/books/\(id)/likes/\(postId)")
        request.httpMethod = on ? "PUT" : "DELETE"
        request.httpBody = Data()
        let set: LikeSet = try await decode(request)
        return set.ids
    }

    func commentIndex(_ hosted: HostedSession, id: String) async throws -> CommentIndex {
        if let commentIndex = hooks.commentIndex { return try await commentIndex(hosted, id) }
        return try await get(hosted, path: "/v1/books/\(id)/comments")
    }

    func comments(_ hosted: HostedSession, id: String, postId: String) async throws -> [PostComment] {
        if let comments = hooks.comments { return try await comments(hosted, id, postId) }
        let thread: CommentThread = try await get(hosted, path: "/v1/books/\(id)/comments/\(postId)")
        return thread.comments
    }

    func addComment(
        _ hosted: HostedSession,
        id: String,
        postId: String,
        text: String,
        commentId: String,
        isPrivate: Bool
    ) async throws -> [PostComment] {
        if let addComment = hooks.addComment {
            return try await addComment(hosted, id, postId, text, commentId, isPrivate)
        }
        var request = try authorized(hosted, path: "/v1/books/\(id)/comments/\(postId)")
        request.httpMethod = "POST"
        request.setValue("application/json; charset=utf-8", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONFiles.encoder.encode(
            NewComment(text: text, id: commentId, isPrivate: isPrivate)
        )
        let thread: CommentThread = try await decode(request)
        return thread.comments
    }

    func deleteComment(_ hosted: HostedSession, id: String, postId: String, commentId: String) async throws -> [PostComment] {
        if let deleteComment = hooks.deleteComment { return try await deleteComment(hosted, id, postId, commentId) }
        var request = try authorized(hosted, path: "/v1/books/\(id)/comments/\(postId)/\(commentId)")
        request.httpMethod = "DELETE"
        let thread: CommentThread = try await decode(request)
        return thread.comments
    }

    func profileReplies(_ hosted: HostedSession, handle: String) async throws -> ProfileReplies {
        let encoded = handle.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? handle
        return try await get(hosted, path: "/v1/profiles/\(encoded)/replies")
    }

    func me(_ hosted: HostedSession) async throws -> Me {
        try await get(hosted, path: "/v1/me")
    }

    func catalog(_ hosted: HostedSession, query: String?, page: Int) async throws -> CatalogPage {
        try await get(hosted, path: catalogSearchPath(query: query, page: page))
    }

    func catalogWork(_ hosted: HostedSession, olid: String) async throws -> WorkDetail {
        try await get(hosted, path: "/v1/catalog/works/\(pathUser(olid))")
    }

    func requestBook(_ hosted: HostedSession, body: NewBookRequest) async throws -> BookRequest {
        var request = try authorized(hosted, path: "/v1/requests")
        request.httpMethod = "POST"
        request.setValue("application/json; charset=utf-8", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONFiles.encoder.encode(body)
        return try await decode(request)
    }

    func cancelRequest(_ hosted: HostedSession, id: String) async throws {
        var request = try authorized(hosted, path: "/v1/requests/\(pathUser(id))")
        request.httpMethod = "DELETE"
        _ = try await expectOk(request)
    }

    func retryRequest(_ hosted: HostedSession, id: String) async throws -> BookRequest {
        var request = try authorized(hosted, path: "/v1/requests/\(pathUser(id))/retry")
        request.httpMethod = "POST"
        request.httpBody = Data()
        return try await decode(request)
    }

    func catalogSearchPath(query: String?, page: Int) -> String {
        var items = [URLQueryItem(name: "page", value: String(max(page, 1)))]
        if let query {
            let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty {
                items.append(URLQueryItem(name: "q", value: trimmed))
            }
        }
        return pathWithQuery("/v1/catalog/search", items)
    }

    func search(_ hosted: HostedSession, query: String, type: String, cursor: String?) async throws -> SearchPage {
        if SearchTab(rawValue: type) == .catalog {
            throw CatalogError(reason: .notSearch, message: "Catalog is not a library search.")
        }
        var items = [
            URLQueryItem(name: "q", value: query),
            URLQueryItem(name: "type", value: type),
        ]
        if let cursor, !cursor.isEmpty {
            items.append(URLQueryItem(name: "cursor", value: cursor))
        }
        return try await get(hosted, path: pathWithQuery("/v1/search", items))
    }

    func profile(_ hosted: HostedSession, username: String) async throws -> ProfileRecord {
        try await get(hosted, path: "/v1/users/\(pathUser(username))")
    }

    func profilePosts(_ hosted: HostedSession, username: String, cursor: String?) async throws -> ProfilePostsPage {
        try await get(hosted, path: paged("/v1/users/\(pathUser(username))/posts", cursor: cursor))
    }

    func profileReposts(_ hosted: HostedSession, username: String, cursor: String?) async throws -> ProfileRepostsPage {
        try await get(hosted, path: paged("/v1/users/\(pathUser(username))/reposts", cursor: cursor))
    }

    func profileBooks(_ hosted: HostedSession, username: String) async throws -> ProfileBooksPage {
        try await get(hosted, path: "/v1/users/\(pathUser(username))/books")
    }

    func feed(_ hosted: HostedSession, cursor: String?) async throws -> HomeFeedPage {
        if let feed = hooks.feed { return try await feed(hosted, cursor) }
        return try await get(hosted, path: paged("/v1/feed", cursor: cursor))
    }

    func setRepost(_ hosted: HostedSession, id: String, postId: String, on: Bool) async throws -> RepostState {
        var request = try authorized(hosted, path: "/v1/books/\(id)/posts/\(postId)/repost")
        request.httpMethod = on ? "PUT" : "DELETE"
        return try await decode(request)
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

    private func pathUser(_ username: String) -> String {
        username.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? username
    }

    private func paged(_ path: String, cursor: String?) -> String {
        guard let cursor, !cursor.isEmpty else { return path }
        return pathWithQuery(path, [URLQueryItem(name: "cursor", value: cursor)])
    }

    private func pathWithQuery(_ path: String, _ items: [URLQueryItem]) -> String {
        var components = URLComponents()
        components.path = path
        components.queryItems = items
        return components.string ?? path
    }

    /// Reads the body in chunks so a known Content-Length can move the shelf ring.
    private func readBody(
        _ request: URLRequest,
        onProgress: @escaping @Sendable (Double?) -> Void
    ) async throws -> (Data, HTTPURLResponse) {
        let reader = BodyReader(onProgress: onProgress)
        let config = session.configuration
        let owned = URLSession(configuration: config, delegate: reader, delegateQueue: nil)
        defer { owned.finishTasksAndInvalidate() }
        return try await reader.start(owned, request)
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
        let (data, response) = try await session.data(for: request)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        if !(200..<300).contains(code) {
            let message = (try? JSONFiles.decoder.decode(HostedErrorBody.self, from: data).error)
                .flatMap { $0.isEmpty ? nil : $0 }
                ?? "Server returned \(code)"
            throw HostedException(message: message, code: code)
        }
        do {
            return try JSONFiles.decoder.decode(T.self, from: data)
        } catch {
            throw HostedException(message: "Could not read the server response.", code: code)
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

/// One posts response. Reports byte progress, and cancels the URLSession task with the caller.
private final class BodyReader: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let onProgress: @Sendable (Double?) -> Void
    private let wait = BodyWait()
    private let lock = NSLock()
    private var data = Data()
    private var expected: Int64 = -1
    private var http: HTTPURLResponse?
    private var ok = false

    init(onProgress: @escaping @Sendable (Double?) -> Void) {
        self.onProgress = onProgress
    }

    func start(_ session: URLSession, _ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        if Task.isCancelled { throw CancellationError() }
        let task = session.dataTask(with: request)
        return try await withTaskCancellationHandler {
            try await wait.value { start in
                if start { task.resume() }
            }
        } onCancel: {
            task.cancel()
            wait.cancel()
        }
    }

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive response: URLResponse,
        completionHandler: @escaping (URLSession.ResponseDisposition) -> Void
    ) {
        let http = response as? HTTPURLResponse
        let code = http?.statusCode ?? 0
        lock.lock()
        self.http = http
        ok = (200..<300).contains(code)
        expected = response.expectedContentLength
        lock.unlock()
        if ok {
            onProgress(expected > 0 ? 0 : nil)
        }
        completionHandler(.allow)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        lock.lock()
        self.data.append(data)
        let count = self.data.count
        let expected = self.expected
        let ok = self.ok
        lock.unlock()
        guard ok else { return }
        if expected > 0 {
            onProgress(min(1, Double(count) / Double(expected)))
        } else {
            onProgress(nil)
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        lock.lock()
        let data = self.data
        let http = self.http
        lock.unlock()
        let result: Result<(Data, HTTPURLResponse), Error>
        if let error {
            result = .failure(error)
        } else if let http {
            result = .success((data, http))
        } else {
            result = .failure(HostedException(message: "Empty download."))
        }
        wait.finish(result)
    }
}

/// The URLSession task can finish, or be cancelled, before the caller is waiting.
/// The result is kept until then. Dropping it leaves the download suspended forever.
final class BodyWait: @unchecked Sendable {
    private enum State {
        case idle
        case waiting(CheckedContinuation<(Data, HTTPURLResponse), Error>)
        case done(Result<(Data, HTTPURLResponse), Error>)
        case cancelled
    }

    private let lock = NSLock()
    private var state = State.idle

    /// `onReady(true)` is the signal to resume the URLSession task. A stored result resumes immediately.
    func value(onReady: (Bool) -> Void) async throws -> (Data, HTTPURLResponse) {
        try await withCheckedThrowingContinuation { cont in
            onReady(register(cont))
        }
    }

    func finish(_ result: Result<(Data, HTTPURLResponse), Error>) {
        let waiting = take(result)
        waiting?.resume(with: result)
    }

    func cancel() {
        let waiting = take(.failure(CancellationError()))
        waiting?.resume(throwing: CancellationError())
    }

    private func register(_ cont: CheckedContinuation<(Data, HTTPURLResponse), Error>) -> Bool {
        lock.lock()
        switch state {
        case .done(let result):
            lock.unlock()
            cont.resume(with: result)
            return false
        case .cancelled:
            lock.unlock()
            cont.resume(throwing: CancellationError())
            return false
        case .idle:
            state = .waiting(cont)
            lock.unlock()
            return true
        case .waiting:
            lock.unlock()
            cont.resume(throwing: CancellationError())
            return false
        }
    }

    /// Stores `result` when nobody is waiting yet. A finish keeps that result. A cancel that lands first wins.
    private func take(_ result: Result<(Data, HTTPURLResponse), Error>) -> CheckedContinuation<(Data, HTTPURLResponse), Error>? {
        lock.lock()
        switch state {
        case .waiting(let cont):
            if case .failure(let error) = result, error is CancellationError {
                state = .cancelled
            } else {
                state = .done(result)
            }
            lock.unlock()
            return cont
        case .idle:
            if case .failure(let error) = result, error is CancellationError {
                state = .cancelled
            } else {
                state = .done(result)
            }
            lock.unlock()
            return nil
        case .done, .cancelled:
            lock.unlock()
            return nil
        }
    }
}
