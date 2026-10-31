import XCTest
@testable import Mewo

final class HostedClientOutboxTests: XCTestCase {
    private var http: OutboxHTTPSession!

    override func setUp() {
        super.setUp()
        OutboxHTTP.reset()
        http = OutboxHTTPSession()
    }

    func testSetLikePutsAndDeletesThePost() async throws {
        let session = signedIn()
        OutboxHTTP.body = Data(#"{"ids":["p"]}"#.utf8)
        let liked = try await http.client.setLike(session, id: book, postId: "p", on: true)
        XCTAssertEqual(liked, ["p"])
        let put = try XCTUnwrap(OutboxHTTP.request)
        XCTAssertEqual(put.httpMethod, "PUT")
        XCTAssertEqual(put.url?.path, "/v1/books/\(book)/likes/p")

        OutboxHTTP.body = Data(#"{"ids":[]}"#.utf8)
        let cleared = try await http.client.setLike(session, id: book, postId: "p", on: false)
        XCTAssertEqual(cleared, [])
        let deleted = try XCTUnwrap(OutboxHTTP.request)
        XCTAssertEqual(deleted.httpMethod, "DELETE")
        XCTAssertEqual(deleted.url?.path, "/v1/books/\(book)/likes/p")
    }

    func testAddCommentSendsALowercaseIdAndPrivateFlag() async throws {
        let session = signedIn()
        OutboxHTTP.body = Data(
            #"{"comments":[{"id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","username":"ada","text":"hi","createdAt":1,"mine":true}]}"#
                .utf8
        )
        let id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        let thread = try await http.client.addComment(
            session,
            id: book,
            postId: "p",
            text: "hi",
            commentId: id,
            isPrivate: true
        )
        XCTAssertEqual(thread.map(\.id), [id])
        let request = try XCTUnwrap(OutboxHTTP.request)
        XCTAssertEqual(request.httpMethod, "POST")
        XCTAssertEqual(request.url?.path, "/v1/books/\(book)/comments/p")
        let body = try JSONFiles.decoder.decode(NewComment.self, from: OutboxHTTP.sent ?? Data())
        XCTAssertEqual(body.text, "hi")
        XCTAssertEqual(body.id, id)
        XCTAssertEqual(body.id, body.id?.lowercased())
        XCTAssertEqual(body.isPrivate, true)
    }

    func testASuccessWithABadBodyThrowsThatStatus() async throws {
        OutboxHTTP.status = 200
        OutboxHTTP.body = Data("not-json".utf8)
        do {
            _ = try await http.client.setLike(signedIn(), id: book, postId: "p", on: true)
            XCTFail("a bad 200 body decoded")
        } catch let error as HostedException {
            XCTAssertEqual(error.code, 200)
        }
    }

    private func signedIn() -> HostedSession {
        HostedSession(baseUrl: "http://127.0.0.1:9", username: "ada", token: "token-a")
    }
}

private let book = "11111111-1111-1111-1111-111111111111"

private final class OutboxHTTP: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var status = 200
    nonisolated(unsafe) static var body = Data()
    nonisolated(unsafe) static var request: URLRequest?
    nonisolated(unsafe) static var sent: Data?

    static func reset() {
        status = 200
        body = Data()
        request = nil
        sent = nil
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.request = request
        Self.sent = Self.bodyData(request)
        let url = request.url ?? URL(string: "http://127.0.0.1")!
        let response = HTTPURLResponse(url: url, statusCode: Self.status, httpVersion: "HTTP/1.1", headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Self.body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}

    private static func bodyData(_ request: URLRequest) -> Data? {
        if let body = request.httpBody { return body }
        guard let stream = request.httpBodyStream else { return nil }
        stream.open()
        defer { stream.close() }
        var data = Data()
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: 1024)
        defer { buffer.deallocate() }
        while stream.hasBytesAvailable {
            let count = stream.read(buffer, maxLength: 1024)
            if count <= 0 { break }
            data.append(buffer, count: count)
        }
        return data
    }
}

private struct OutboxHTTPSession {
    let client: HostedClient

    init() {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [OutboxHTTP.self]
        client = HostedClient(session: URLSession(configuration: config))
    }
}
