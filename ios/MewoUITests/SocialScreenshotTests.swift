import XCTest

/// Walks the social surfaces against Fixtures/social-fixture.py on 127.0.0.1:8766.
/// Skips when that process is not listening. Shots land in /tmp/mewo-social-shots/<theme>/.
final class SocialScreenshotTests: XCTestCase {
    func testLightSurfaces() throws {
        try walk(theme: "x_light")
    }

    func testDarkSurfaces() throws {
        try walk(theme: "x_lights_out")
    }

    private func walk(theme: String) throws {
        guard fixtureReady() else {
            XCTFail("social fixture is not reachable on 127.0.0.1:8766")
            return
        }
        let app = XCUIApplication()
        app.launchArguments = [
            "--uitesting",
            "--hosted-url=http://127.0.0.1:8766",
            "--hosted-user=ada",
            "--hosted-token=token",
            "--profile-name=Ada",
            "--profile-handle=ada",
            "--theme=\(theme)",
            "--seed-shelf",
            "--seed-feed",
        ]
        app.launch()

        let reposted = app.staticTexts["bea reposted"]
        XCTAssertTrue(reposted.waitForExistence(timeout: 12), "cached feed did not paint")
        shot(app, theme, "home-feed")

        app.buttons["account-button"].tap()
        XCTAssertTrue(app.buttons["drawer-profile"].waitForExistence(timeout: 5))
        shot(app, theme, "drawer")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.92, dy: 0.4)).tap()
        XCTAssertTrue(app.buttons["home-books"].waitForExistence(timeout: 5))

        app.buttons["home-books"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        XCTAssertTrue(app.staticTexts["A Timeline of My Own"].waitForExistence(timeout: 8))
        shot(app, theme, "home-books")

        app.buttons["tab-search"].tap()
        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.tap()
        field.typeText("ada")
        XCTAssertTrue(app.buttons["account-ada"].waitForExistence(timeout: 8))
        dismissKeyboard(app)
        shot(app, theme, "search-accounts")

        app.buttons["search-tab-books"].tap()
        XCTAssertTrue(app.staticTexts["A Timeline of My Own"].waitForExistence(timeout: 8))
        shot(app, theme, "search-books")

        app.buttons["search-tab-posts"].tap()
        XCTAssertTrue(app.staticTexts["The first sentence is the one you remember."].waitForExistence(timeout: 8))
        shot(app, theme, "search-posts")

        app.buttons["search-tab-accounts"].tap()
        XCTAssertTrue(app.buttons["account-ada"].waitForExistence(timeout: 5))
        app.buttons["account-ada"].tap()
        XCTAssertTrue(app.staticTexts["Joined Mar 2024"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["Still true."].waitForExistence(timeout: 8))
        shot(app, theme, "profile-posts")

        app.buttons["profile-tab-reposts"].tap()
        XCTAssertTrue(app.staticTexts["The first sentence is the one you remember."].waitForExistence(timeout: 8))
        shot(app, theme, "profile-reposts")

        app.buttons["profile-tab-books"].tap()
        XCTAssertTrue(app.staticTexts["A Timeline of My Own"].waitForExistence(timeout: 8))
        shot(app, theme, "profile-books")

        app.buttons["Back"].tap()
        app.buttons["tab-home"].tap()
        app.buttons["home-books"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        app.staticTexts["A Timeline of My Own"].tap()
        let reposts = app.buttons["Reposted, 3"]
        XCTAssertTrue(reposts.waitForExistence(timeout: 12), "reposted post did not paint")
        shot(app, theme, "repost-active")

        app.buttons["Back"].tap()
        app.buttons["home-feed"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        XCTAssertTrue(app.staticTexts["bea reposted"].waitForExistence(timeout: 8))
        let scroll = app.scrollViews["feed-scroll"]
        XCTAssertTrue(scroll.waitForExistence(timeout: 5))
        let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.25))
        let end = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.8))
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: XCUIGestureVelocity(rawValue: 40), thenHoldForDuration: 1.6)
        Thread.sleep(forTimeInterval: 0.4)
        shot(app, theme, "feed-pull")
    }

    private func shot(_ app: XCUIApplication, _ theme: String, _ name: String) {
        let dir = URL(fileURLWithPath: "/tmp/mewo-social-shots/\(theme)", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent("\(name).png")
        try? XCUIScreen.main.screenshot().pngRepresentation.write(to: file)
    }

    private func dismissKeyboard(_ app: XCUIApplication) {
        guard app.keyboards.element.exists else { return }
        if app.keyboards.buttons["Search"].exists {
            app.keyboards.buttons["Search"].tap()
        } else if app.keyboards.buttons["return"].exists {
            app.keyboards.buttons["return"].tap()
        }
    }

    private func fixtureReady() -> Bool {
        let status = URL(string: "http://127.0.0.1:8766/control/status")!
        let probe = DispatchSemaphore(value: 0)
        var listening = false
        URLSession.shared.dataTask(with: status) { data, response, _ in
            if let http = response as? HTTPURLResponse, http.statusCode == 200,
               let data, let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               json["feed_requests"] != nil {
                listening = true
            }
            probe.signal()
        }.resume()
        _ = probe.wait(timeout: .now() + 2)
        guard listening else { return false }
        var hold = URLRequest(url: URL(string: "http://127.0.0.1:8766/control/hold")!)
        hold.httpMethod = "POST"
        let held = DispatchSemaphore(value: 0)
        URLSession.shared.dataTask(with: hold) { _, _, _ in held.signal() }.resume()
        _ = held.wait(timeout: .now() + 2)
        return true
    }
}
