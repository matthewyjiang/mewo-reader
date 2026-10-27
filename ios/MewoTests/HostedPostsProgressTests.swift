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
