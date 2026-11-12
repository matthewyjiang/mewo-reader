import XCTest
@testable import Mewo

final class EmailDecodingTests: XCTestCase {
    func testMeReadsEmailFieldsAndOlderServersOmitThem() throws {
        let me = try decodeMe(
            #"{"username":"ada","isAdmin":false,"canRequest":true,"email":null,"pendingEmail":"new@example.com","passwordReset":true}"#
        )
        XCTAssertNil(me.email)
        XCTAssertEqual(me.pendingEmail, "new@example.com")
        XCTAssertTrue(me.passwordReset)

        let old = try decodeMe(#"{"username":"ada","isAdmin":false,"canRequest":true}"#)
        XCTAssertEqual(old, Me(username: "ada", isAdmin: false, canRequest: true))
        XCTAssertFalse(old.passwordReset)
    }

    func testInstanceReadsPasswordReset() throws {
        let on = try JSONFiles.decoder.decode(
            InstanceInfo.self,
            from: Data(#"{"allowSignup":true,"passwordReset":true}"#.utf8)
        )
        XCTAssertEqual(on, InstanceInfo(allowSignup: true, passwordReset: true))
        let old = try JSONFiles.decoder.decode(InstanceInfo.self, from: Data(#"{"allowSignup":false}"#.utf8))
        XCTAssertFalse(old.passwordReset)
    }

    func testEmailTrailPrefersPending() {
        let base = Me(username: "ada", isAdmin: false, canRequest: false)
        XCTAssertEqual(emailTrail(base), "Add")
        var verified = base
        verified.email = "ada@example.com"
        XCTAssertEqual(emailTrail(verified), "ada@example.com")
        var pending = verified
        pending.pendingEmail = "new@example.com"
        XCTAssertEqual(emailTrail(pending), "Check inbox")
        XCTAssertEqual(emailSentNotice(" new@example.com "), "Check new@example.com for a link.")
    }

    private func decodeMe(_ json: String) throws -> Me {
        try JSONFiles.decoder.decode(Me.self, from: Data(json.utf8))
    }
}

final class EmailClientTests: XCTestCase {
    override func tearDown() {
        EmailHTTP.reset()
        super.tearDown()
    }

    func testRegisterSendsEmailOnlyWhenGiven() async throws {
        let client = EmailHTTP.client()
        EmailHTTP.body = Data(#"{"token":"t","username":"ada"}"#.utf8)
        _ = try await client.register(baseUrl: "http://127.0.0.1:9", username: "ada", password: "password1", email: " ada@example.com ")
        XCTAssertEqual(EmailHTTP.requests.last?.url?.path, "/v1/auth/register")
        XCTAssertEqual(EmailHTTP.json()["email"] as? String, "ada@example.com")

        _ = try await client.register(baseUrl: "http://127.0.0.1:9", username: "ada", password: "password1", email: "  ")
        XCTAssertNil(EmailHTTP.json()["email"])
        XCTAssertEqual(EmailHTTP.json()["username"] as? String, "ada")
    }

    func testRequestResetPostsTheLoginWithoutASession() async throws {
        EmailHTTP.status = 202
        try await EmailHTTP.client().requestReset(baseUrl: "http://127.0.0.1:9", login: " ada ")
        let request = EmailHTTP.requests.last
        XCTAssertEqual(request?.httpMethod, "POST")
        XCTAssertEqual(request?.url?.path, "/v1/auth/reset/request")
        XCTAssertNil(request?.value(forHTTPHeaderField: "Authorization"))
        XCTAssertEqual(EmailHTTP.json() as NSDictionary, ["login": "ada"])
    }

    func testSetEmailReportsWhetherALinkWentOut() async throws {
        let client = EmailHTTP.client()
        let session = HostedSession(baseUrl: "http://127.0.0.1:9", username: "ada", token: "token")

        EmailHTTP.status = 202
        let sent = try await client.setEmail(session, email: "ada@example.com", currentPassword: "password1")
        XCTAssertTrue(sent)
        let put = EmailHTTP.requests.last
        XCTAssertEqual(put?.httpMethod, "PUT")
        XCTAssertEqual(put?.url?.path, "/v1/me/email")
        XCTAssertEqual(put?.value(forHTTPHeaderField: "Authorization"), "Bearer token")
        XCTAssertEqual(
            EmailHTTP.json() as NSDictionary,
            ["email": "ada@example.com", "currentPassword": "password1"]
        )

        EmailHTTP.status = 204
        let removed = try await client.setEmail(session, email: nil, currentPassword: "password1")
        XCTAssertFalse(removed)
        XCTAssertTrue(EmailHTTP.json()["email"] is NSNull)
    }

    func testSetEmailRaisesTheServerMessage() async {
        EmailHTTP.status = 403
        EmailHTTP.body = Data(#"{"error":"Wrong password."}"#.utf8)
        let session = HostedSession(baseUrl: "http://127.0.0.1:9", username: "ada", token: "token")
        do {
            _ = try await EmailHTTP.client().setEmail(session, email: "ada@example.com", currentPassword: "nope")
            XCTFail("expected an error")
        } catch let error as HostedException {
            XCTAssertEqual(error.message, "Wrong password.")
            XCTAssertEqual(error.code, 403)
        } catch {
            XCTFail("unexpected \(error)")
        }
    }
}

private final class EmailHTTP: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var status = 200
    nonisolated(unsafe) static var body = Data()
    nonisolated(unsafe) static var requests: [URLRequest] = []
    nonisolated(unsafe) static var bodies: [Data] = []

    static func client() -> HostedClient {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [EmailHTTP.self]
        return HostedClient(session: URLSession(configuration: config))
    }

    static func reset() {
        status = 200
        body = Data()
        requests = []
        bodies = []
    }

    /// The last request body as a JSON object.
    static func json() -> [String: Any] {
        guard let data = bodies.last,
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return [:] }
        return object
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.requests.append(request)
        Self.bodies.append(Self.read(request))
        let url = request.url ?? URL(string: "http://127.0.0.1")!
        let response = HTTPURLResponse(url: url, statusCode: Self.status, httpVersion: "HTTP/1.1", headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Self.body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}

    /// URLSession hands a protocol the body as a stream.
    private static func read(_ request: URLRequest) -> Data {
        if let body = request.httpBody { return body }
        guard let stream = request.httpBodyStream else { return Data() }
        stream.open()
        defer { stream.close() }
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while stream.hasBytesAvailable {
            let count = stream.read(&buffer, maxLength: buffer.count)
            if count <= 0 { break }
            data.append(buffer, count: count)
        }
        return data
    }
}
