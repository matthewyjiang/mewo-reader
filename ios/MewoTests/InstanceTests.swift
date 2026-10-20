import XCTest
@testable import Mewo

final class InstanceDecodingTests: XCTestCase {
    func testDecodesAllowSignupAndIgnoresUnknownKeys() throws {
        let closed = try decode(#"{"allowSignup":false,"name":"later"}"#)
        XCTAssertEqual(closed, InstanceInfo(allowSignup: false))
        let open = try decode(#"{"allowSignup":true,"name":"later"}"#)
        XCTAssertEqual(open, InstanceInfo(allowSignup: true))
    }

    func testMissingOrNullSignupStaysOpen() throws {
        XCTAssertEqual(try decode("{}").allowSignup, true)
        XCTAssertEqual(try decode(#"{"name":"later"}"#).allowSignup, true)
        XCTAssertEqual(try decode(#"{"allowSignup":null,"name":"later"}"#).allowSignup, true)
    }

    func testRejectsANonBooleanSignup() {
        XCTAssertThrowsError(try decode(#"{"allowSignup":"no"}"#))
        XCTAssertThrowsError(try decode(#"{"allowSignup":0}"#))
        XCTAssertThrowsError(try decode("[]"))
    }

    private func decode(_ json: String) throws -> InstanceInfo {
        try JSONFiles.decoder.decode(InstanceInfo.self, from: Data(json.utf8))
    }
}

@MainActor
final class InstanceSignupTests: XCTestCase {
    override func tearDown() {
        InstanceHTTP.status = 200
        InstanceHTTP.body = Data()
        InstanceHTTP.request = nil
        super.tearDown()
    }

    func testClosedHidesAndOpenOrFailedShows() async {
        let http = InstanceHTTPSession()
        let loader = InstanceSignupLoader(debounceNanos: 0) { url in
            try await http.client.instance(baseUrl: url)
        }

        await respond(loader, status: 200, body: #"{"allowSignup":true,"name":"later"}"#)
        XCTAssertEqual(loader.allowSignup, true)
        XCTAssertTrue(showCreateAccount(loader.allowSignup))
        let request = InstanceHTTP.request
        XCTAssertEqual(request?.httpMethod, "GET")
        XCTAssertEqual(request?.url?.path, "/v1/instance")
        XCTAssertNil(request?.value(forHTTPHeaderField: "Authorization"))

        await respond(loader, status: 200, body: #"{"allowSignup":false,"name":"later"}"#)
        XCTAssertEqual(loader.allowSignup, false)
        XCTAssertFalse(showCreateAccount(loader.allowSignup))

        await respond(loader, status: 500, body: #"{"error":"down"}"#)
        XCTAssertNil(loader.allowSignup)
        XCTAssertTrue(showCreateAccount(loader.allowSignup))

        let thrown = InstanceSignupLoader(debounceNanos: 0) { _ in
            throw HostedException(message: "offline", code: 0)
        }
        thrown.noteUrl("http://127.0.0.1:9")
        await thrown.settle()
        XCTAssertNil(thrown.allowSignup)
        XCTAssertTrue(showCreateAccount(thrown.allowSignup))
    }

    func testNotFoundKeepsTheCreateLink() async {
        let http = InstanceHTTPSession()
        let loader = InstanceSignupLoader(debounceNanos: 0) { url in
            try await http.client.instance(baseUrl: url)
        }
        // An old server 404s. The body must not be read as a closed signup.
        await respond(loader, status: 404, body: #"{"allowSignup":false}"#)
        XCTAssertNil(loader.allowSignup)
        XCTAssertTrue(showCreateAccount(loader.allowSignup))
        XCTAssertEqual(InstanceHTTP.request?.url?.path, "/v1/instance")
    }

    func testTypingPauseIsFourHundredMilliseconds() {
        XCTAssertEqual(instanceSignupDebounceNanos, 400_000_000)
    }

    func testBlankUrlDoesNotFetch() async {
        let seen = URLBox()
        let loader = InstanceSignupLoader(debounceNanos: 0) { url in
            await seen.add(url)
            return InstanceInfo(allowSignup: false)
        }
        loader.noteUrl("   ")
        await loader.settle()
        XCTAssertNil(loader.allowSignup)
        XCTAssertTrue(showCreateAccount(loader.allowSignup))
        let urls = await seen.urls()
        XCTAssertEqual(urls, [])
    }

    func testNewerUrlCancelsTheOlderWait() async {
        let gate = PauseGate()
        let seen = URLBox()
        let loader = InstanceSignupLoader(
            debounceNanos: 1,
            fetch: { url in
                await seen.add(url)
                return InstanceInfo(allowSignup: url.contains("new"))
            },
            pause: { try await gate.pause() }
        )
        loader.noteUrl("http://old.example")
        await gate.untilWaiting(1)
        loader.noteUrl("http://new.example")
        await gate.untilWaiting(2)
        let before = await seen.urls()
        XCTAssertEqual(before, [])
        await gate.release()
        await loader.settle()
        let after = await seen.urls()
        XCTAssertEqual(after, ["http://new.example"])
        XCTAssertEqual(loader.allowSignup, true)
    }

    func testStaleResponseCannotPublish() async {
        let gate = SignupGate()
        let cancelled = Flag()
        let loader = InstanceSignupLoader(debounceNanos: 0) { url in
            await gate.enter(url)
            if url.contains("old") {
                await cancelled.set(Task.isCancelled)
            }
            await gate.finish(url)
            return InstanceInfo(allowSignup: url.contains("new"))
        }
        loader.noteUrl("http://old.example")
        await gate.untilArrived("http://old.example")
        loader.noteUrl("http://new.example")
        await gate.untilArrived("http://new.example")
        await gate.release("http://new.example")
        await loader.settle()
        XCTAssertEqual(loader.allowSignup, true)

        await gate.release("http://old.example")
        await gate.untilFinished("http://old.example")
        try? await Task.sleep(nanoseconds: 30_000_000)
        XCTAssertEqual(loader.allowSignup, true)
        XCTAssertTrue(showCreateAccount(loader.allowSignup))
        let oldWasCancelled = await cancelled.get()
        XCTAssertTrue(oldWasCancelled)
    }

    private func respond(_ loader: InstanceSignupLoader, status: Int, body: String) async {
        InstanceHTTP.status = status
        InstanceHTTP.body = Data(body.utf8)
        loader.noteUrl("http://127.0.0.1:9")
        await loader.settle()
    }
}

private final class InstanceHTTP: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var status = 200
    nonisolated(unsafe) static var body = Data()
    nonisolated(unsafe) static var request: URLRequest?

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.request = request
        let url = request.url ?? URL(string: "http://127.0.0.1")!
        let response = HTTPURLResponse(
            url: url,
            statusCode: Self.status,
            httpVersion: "HTTP/1.1",
            headerFields: nil
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Self.body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

private struct InstanceHTTPSession {
    let client: HostedClient

    init() {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [InstanceHTTP.self]
        client = HostedClient(session: URLSession(configuration: config))
    }
}

private actor SignupGate {
    private var arrived: Set<String> = []
    private var arriveWaiters: [String: [CheckedContinuation<Void, Never>]] = [:]
    private var parked: [String: CheckedContinuation<Void, Never>] = [:]
    private var finished: Set<String> = []
    private var finishWaiters: [String: [CheckedContinuation<Void, Never>]] = [:]

    func enter(_ url: String) async {
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            parked[url] = cont
            arrived.insert(url)
            arriveWaiters.removeValue(forKey: url)?.forEach { $0.resume() }
        }
    }

    func untilArrived(_ url: String) async {
        if arrived.contains(url) { return }
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            var pending = arriveWaiters[url] ?? []
            pending.append(cont)
            arriveWaiters[url] = pending
        }
    }

    func release(_ url: String) {
        parked.removeValue(forKey: url)?.resume()
    }

    func finish(_ url: String) {
        finished.insert(url)
        finishWaiters.removeValue(forKey: url)?.forEach { $0.resume() }
    }

    func untilFinished(_ url: String) async {
        if finished.contains(url) { return }
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            var pending = finishWaiters[url] ?? []
            pending.append(cont)
            finishWaiters[url] = pending
        }
    }
}

private final class PauseGate: @unchecked Sendable {
    private let lock = NSLock()
    private var continuations: [CheckedContinuation<Void, Error>] = []
    private var waiting = 0
    private var watchers: [CheckedContinuation<Void, Never>] = []

    func pause() async throws {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            lock.lock()
            continuations.append(cont)
            waiting += 1
            let watchers = self.watchers
            self.watchers.removeAll()
            lock.unlock()
            watchers.forEach { $0.resume() }
        }
    }

    func untilWaiting(_ count: Int) async {
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            lock.lock()
            if waiting >= count {
                lock.unlock()
                cont.resume()
                return
            }
            watchers.append(cont)
            lock.unlock()
        }
    }

    /// Earlier waits fail the way `Task.sleep` does when that task is cancelled.
    func release() {
        lock.lock()
        let all = continuations
        continuations.removeAll()
        lock.unlock()
        for (index, cont) in all.enumerated() {
            if index < all.count - 1 {
                cont.resume(throwing: CancellationError())
            } else {
                cont.resume()
            }
        }
    }
}

private actor URLBox {
    private var values: [String] = []

    func add(_ url: String) {
        values.append(url)
    }

    func urls() -> [String] {
        values
    }
}

private actor Flag {
    private var value = false

    func set(_ value: Bool) {
        self.value = value
    }

    func get() -> Bool {
        value
    }
}
