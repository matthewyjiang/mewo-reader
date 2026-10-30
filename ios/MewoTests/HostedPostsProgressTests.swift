import XCTest
@testable import Mewo

final class HostedPostsProgressTests: XCTestCase {
    func testPostsReportsByteProgressWhenTheLengthIsKnown() async throws {
        let body = Data(#"{"posts":[{"id":"p","kind":"Body","text":"Hello","chapter":"1"}]}"#.utf8)
        let seen = ProgressLog()
        let posts = try await fetchPosts(body: body, headers: ["Content-Length": "\(body.count)"], seen: seen)
        XCTAssertEqual(posts.map(\.text), ["Hello"])
        let values = seen.values
        XCTAssertFalse(values.isEmpty)
        XCTAssertTrue(values.allSatisfy { $0 != nil })
        XCTAssertEqual(values.first ?? nil, 0)
        XCTAssertEqual(values.last ?? nil, 1)
    }

    func testAResultThatArrivesBeforeTheWaiterResumes() async {
        let wait = BodyWait()
        let response = HTTPURLResponse(
            url: URL(string: "https://mewo.example/posts")!,
            statusCode: 200,
            httpVersion: nil,
            headerFields: nil
        )!
        wait.finish(.success((Data("ok".utf8), response)))
        let task = Task<(Data, HTTPURLResponse), Error> {
            try await wait.value { _ in }
        }
        let result = await outcome(task, within: 2)
        guard case .value(let data, let http) = result else {
            return XCTFail("stored result did not resume, got \(result)")
        }
        XCTAssertEqual(data, Data("ok".utf8))
        XCTAssertEqual(http, 200)
    }

    func testAFinishWhileWaitingKeepsTheResult() async throws {
        let wait = BodyWait()
        let response = HTTPURLResponse(
            url: URL(string: "https://mewo.example/posts")!,
            statusCode: 200,
            httpVersion: nil,
            headerFields: nil
        )!
        let body = Data("ok".utf8)
        let first = try await wait.value { start in
            if start { wait.finish(.success((body, response))) }
        }
        XCTAssertEqual(first.0, body)
        let again = try await wait.value { _ in }
        XCTAssertEqual(again.0, body)
        XCTAssertEqual(again.1.statusCode, 200)
    }

    func testACancelThatArrivesBeforeTheWaiterResumes() async {
        let wait = BodyWait()
        wait.cancel()
        let task = Task<(Data, HTTPURLResponse), Error> {
            try await wait.value { _ in }
        }
        let result = await outcome(task, within: 2)
        guard case .cancelled = result else {
            return XCTFail("cancel before the waiter hung or failed as \(result)")
        }
    }

    func testCancelBeforeTheRequestStartsFinishes() async {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [SilentPostsProtocol.self]
        let client = HostedClient(session: URLSession(configuration: config))
        let session = HostedSession(baseUrl: "https://mewo.example", username: "ada", token: "token")
        for _ in 0..<200 {
            let task = Task<[FeedPost], Error> {
                try await client.posts(session, id: "b1")
            }
            task.cancel()
            let result = await outcome(task, within: 2)
            guard case .cancelled = result else {
                return XCTFail("cancel before posts started did not finish, got \(result)")
            }
        }
    }

    func testCancelMidFlightFinishes() async {
        SilentPostsProtocol.reset()
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [SilentPostsProtocol.self]
        let client = HostedClient(session: URLSession(configuration: config))
        let session = HostedSession(baseUrl: "https://mewo.example", username: "ada", token: "token")
        let task = Task<[FeedPost], Error> {
            try await client.posts(session, id: "b1")
        }
        let deadline = Date().addingTimeInterval(2)
        while SilentPostsProtocol.started == 0 {
            if Date() > deadline {
                task.cancel()
                return XCTFail("posts request never started")
            }
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
        task.cancel()
        let result = await outcome(task, within: 2)
        guard case .cancelled = result else {
            return XCTFail("mid-flight cancel did not finish, got \(result)")
        }
    }

    func testPostsStaysIndeterminateWithoutALength() async throws {
        let body = Data(#"{"posts":[{"id":"p","kind":"Body","text":"Hello","chapter":"1"}]}"#.utf8)
        let seen = ProgressLog()
        _ = try await fetchPosts(body: body, headers: [:], seen: seen)
        let values = seen.values
        XCTAssertFalse(values.isEmpty)
        XCTAssertTrue(values.allSatisfy { $0 == nil })
    }

    private func fetchPosts(body: Data, headers: [String: String], seen: ProgressLog) async throws -> [FeedPost] {
        PostsBodyProtocol.install(body: body, headers: headers)
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [PostsBodyProtocol.self]
        let client = HostedClient(session: URLSession(configuration: config))
        let session = HostedSession(baseUrl: "https://mewo.example", username: "ada", token: "token")
        return try await client.posts(session, id: "b1") { fraction in
            seen.add(fraction)
        }
    }
}

private final class ProgressLog: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [Double?] = []

    func add(_ value: Double?) {
        lock.lock()
        stored.append(value)
        lock.unlock()
    }

    var values: [Double?] {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }
}

private enum CallOutcome: Equatable {
    case hung
    case cancelled
    case value(Data, Int)
    case error(String)
}

/// How `task` finished. A hang is `.hung` so the suite does not wait forever.
private func outcome<T>(_ task: Task<T, Error>, within seconds: Double) async -> CallOutcome {
    await withCheckedContinuation { (cont: CheckedContinuation<CallOutcome, Never>) in
        let box = FinishBox()
        Task {
            do {
                let value = try await task.value
                box.finish(describe(value), cont)
            } catch {
                box.finish(describe(error), cont)
            }
        }
        DispatchQueue.global().asyncAfter(deadline: .now() + seconds) {
            box.finish(.hung, cont)
        }
    }
}

private func describe<T>(_ value: T) -> CallOutcome {
    if let pair = value as? (Data, HTTPURLResponse) {
        return .value(pair.0, pair.1.statusCode)
    }
    return .error(String(describing: value))
}

private func describe(_ error: Error) -> CallOutcome {
    if error is CancellationError { return .cancelled }
    if let url = error as? URLError, url.code == .cancelled { return .cancelled }
    return .error(String(describing: error))
}

private final class FinishBox: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false

    func finish(_ outcome: CallOutcome, _ cont: CheckedContinuation<CallOutcome, Never>) {
        lock.lock()
        if done {
            lock.unlock()
            return
        }
        done = true
        lock.unlock()
        cont.resume(returning: outcome)
    }
}

/// Never completes a response, so a cancelled download can only finish through cancellation.
private final class SilentPostsProtocol: URLProtocol, @unchecked Sendable {
    static let lock = NSLock()
    private static var count = 0

    static var started: Int {
        lock.lock()
        defer { lock.unlock() }
        return count
    }

    static func reset() {
        lock.lock()
        count = 0
        lock.unlock()
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.lock.lock()
        Self.count += 1
        Self.lock.unlock()
    }

    override func stopLoading() {}
}

private final class PostsBodyProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    private static var body = Data()
    private static var headers: [String: String] = [:]

    static func install(body: Data, headers: [String: String]) {
        lock.lock()
        self.body = body
        self.headers = headers
        lock.unlock()
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.lock.lock()
        let body = Self.body
        let headers = Self.headers
        Self.lock.unlock()
        let url = request.url ?? URL(string: "https://mewo.example/v1/books/b1/posts")!
        let response = HTTPURLResponse(url: url, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        if !body.isEmpty {
            client?.urlProtocol(self, didLoad: body)
        }
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
