import XCTest

/// Catalog tab and detail sheet against Fixtures/social-fixture.py on 127.0.0.1:8766.
/// The iOS job does not run UI tests. A Mac with the fixture has to take these shots.
final class CatalogScreenshotTests: XCTestCase {
    func testCatalogTrendingAndDetail() throws {
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
            "--theme=x_light",
            "--seed-shelf",
            "--seed-feed",
        ]
        app.launch()

        app.buttons["tab-search"].tap()
        XCTAssertTrue(app.buttons["search-tab-catalog"].waitForExistence(timeout: 8))
        dismissKeyboard(app)
        app.buttons["search-tab-catalog"].tap()
        XCTAssertTrue(app.staticTexts["Trending on Open Library"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["Dune"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["The Left Hand of Darkness"].exists)
        dismissKeyboard(app)
        shot(app, "catalog-trending")

        app.buttons["catalog-OL1W"].tap()
        XCTAssertTrue(app.staticTexts["Looking for a release…"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["catalog-cancel"].waitForExistence(timeout: 8))
        shot(app, "catalog-detail")
    }

    private func shot(_ app: XCUIApplication, _ name: String) {
        let dir = URL(fileURLWithPath: "/tmp/mewo-catalog-shots", isDirectory: true)
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
