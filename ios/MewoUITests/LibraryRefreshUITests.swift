import XCTest

/// Pulls the hosted shelf while a request is held, so the system spinner stays up.
/// The hold server is `Fixtures/shelf-delay.py` on 127.0.0.1:8765. Skip when it is down
/// so `xcodebuild test -scheme Mewo` still passes.
final class LibraryRefreshUITests: XCTestCase {
    func testPullKeepsTheShelfAndTheSpinner() throws {
        try pull(theme: "x_light", shot: "/tmp/mewo-pull-held-light.png")
    }

    func testPullKeepsTheShelfInLightsOut() throws {
        try pull(theme: "x_lights_out", shot: "/tmp/mewo-pull-held-dark.png")
    }

    private func pull(theme: String, shot: String) throws {
        try XCTSkipUnless(holdServerReady(), "shelf hold server is not reachable on 127.0.0.1:8765")
        let app = XCUIApplication()
        app.launchArguments = [
            "--uitesting",
            "--hosted-url=http://127.0.0.1:8765",
            "--hosted-user=ada",
            "--hosted-token=token",
            "--profile-name=Ada",
            "--profile-handle=ada",
            "--theme=\(theme)",
            "--seed-shelf",
        ]
        app.launch()

        let books = app.buttons["home-books"]
        XCTAssertTrue(books.waitForExistence(timeout: 8))
        books.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        let title = app.staticTexts["A Timeline of My Own"]
        XCTAssertTrue(title.waitForExistence(timeout: 8), "cached shelf did not paint")
        let scroll = app.scrollViews["library-scroll"]
        XCTAssertTrue(scroll.waitForExistence(timeout: 3))

        let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.2))
        let end = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.75))
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: XCUIGestureVelocity(rawValue: 40), thenHoldForDuration: 1.4)

        // The refreshable closure is still awaiting the held server, so the spinner stays.
        Thread.sleep(forTimeInterval: 0.3)
        XCTAssertTrue(title.exists, "pull hid the shelf")
        try XCUIScreen.main.screenshot().pngRepresentation.write(to: URL(fileURLWithPath: shot))

        let release = URL(string: "http://127.0.0.1:8765/control/release")!
        let done = expectation(description: "release")
        URLSession.shared.dataTask(with: release) { _, _, _ in done.fulfill() }.resume()
        wait(for: [done], timeout: 5)
        // The next pull needs the server held again.
        let hold = URL(string: "http://127.0.0.1:8765/control/hold")!
        let held = expectation(description: "hold")
        URLSession.shared.dataTask(with: hold) { _, _, _ in held.fulfill() }.resume()
        wait(for: [held], timeout: 5)
        XCTAssertTrue(title.waitForExistence(timeout: 8))
    }

    /// True when Fixtures/shelf-delay.py (or anything speaking its control routes) is listening.
    private func holdServerReady() -> Bool {
        let status = URL(string: "http://127.0.0.1:8765/control/status")!
        let probe = DispatchSemaphore(value: 0)
        var listening = false
        URLSession.shared.dataTask(with: status) { data, response, _ in
            if let http = response as? HTTPURLResponse, http.statusCode == 200,
               let data, let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               json["waiting"] != nil {
                listening = true
            }
            probe.signal()
        }.resume()
        _ = probe.wait(timeout: .now() + 2)
        guard listening else { return false }
        var hold = URLRequest(url: URL(string: "http://127.0.0.1:8765/control/hold")!)
        hold.httpMethod = "POST"
        let held = DispatchSemaphore(value: 0)
        URLSession.shared.dataTask(with: hold) { _, _, _ in held.signal() }.resume()
        _ = held.wait(timeout: .now() + 2)
        return true
    }
}
