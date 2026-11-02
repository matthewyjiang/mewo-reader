import XCTest

/// Browse states for Accounts, Books and Posts against Fixtures/social-fixture.py on 127.0.0.1:8766.
/// The iOS job does not run UI tests. Shots land in /tmp/mewo-browse-shots.
final class SearchBrowseScreenshotTests: XCTestCase {
    func testBrowseStates() throws {
        guard fixtureReady() else {
            XCTFail("social fixture is not reachable on 127.0.0.1:8766")
            return
        }
        setLibraryMode("rows")
        let app = launch()
        openSearch(app)
        XCTAssertTrue(app.staticTexts["Suggested accounts"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["account-ada"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["account-bea"].exists)
        dismissKeyboard(app)
        shot(app, "browse-accounts")

        app.buttons["search-tab-books"].tap()
        XCTAssertTrue(app.staticTexts["Popular on this server"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["A Timeline of My Own"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["Emma"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["Shared by @bea"].exists)
        dismissKeyboard(app)
        shot(app, "browse-books")

        app.buttons["search-tab-posts"].tap()
        XCTAssertTrue(app.staticTexts["Top posts"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["The first sentence is the one you remember."].waitForExistence(timeout: 8))
        dismissKeyboard(app)
        shot(app, "browse-posts")

        let field = app.textFields["search-field"]
        field.tap()
        field.typeText("zzzz")
        XCTAssertTrue(app.staticTexts["No posts match “zzzz”"].waitForExistence(timeout: 8))
        dismissKeyboard(app)
        shot(app, "posts-no-matches")

        app.buttons["search-tab-books"].tap()
        XCTAssertTrue(app.staticTexts["No books match “zzzz”"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["Search the Catalog"].waitForExistence(timeout: 8))
        dismissKeyboard(app)
        shot(app, "books-no-matches")

        setLibraryMode("empty")
        let empty = launch()
        openSearch(empty)
        XCTAssertTrue(empty.staticTexts["It's just you here so far."].waitForExistence(timeout: 8))
        XCTAssertTrue(empty.staticTexts["Suggested accounts"].exists)
        dismissKeyboard(empty)
        shot(empty, "accounts-nothing-yet")

        empty.buttons["search-tab-books"].tap()
        XCTAssertTrue(empty.staticTexts["No books yet."].waitForExistence(timeout: 8))
        XCTAssertTrue(empty.buttons["Browse the Catalog"].exists)
        dismissKeyboard(empty)
        shot(empty, "books-nothing-yet")
        empty.buttons["Browse the Catalog"].tap()
        XCTAssertTrue(empty.staticTexts["Trending on Open Library"].waitForExistence(timeout: 8))

        empty.buttons["search-tab-posts"].tap()
        let clear = empty.buttons["search-clear"]
        if clear.exists { clear.tap() }
        XCTAssertTrue(empty.staticTexts["No posts yet. Replies and reposts on books show up here."].waitForExistence(timeout: 8))
        XCTAssertTrue(empty.buttons["Go to your books"].exists)
        dismissKeyboard(empty)
        shot(empty, "posts-nothing-yet")
        empty.buttons["Go to your books"].tap()
        XCTAssertTrue(empty.staticTexts["bea reposted"].waitForExistence(timeout: 8))
        shot(empty, "posts-go-home")

        setLibraryMode("fail")
        let failed = launch()
        openSearch(failed)
        XCTAssertTrue(failed.staticTexts["Search failed."].waitForExistence(timeout: 8))
        XCTAssertTrue(failed.buttons["Try again"].exists)
        XCTAssertTrue(failed.staticTexts["Suggested accounts"].exists)
        dismissKeyboard(failed)
        shot(failed, "browse-error")
        setLibraryMode("rows")
    }

    private func launch() -> XCUIApplication {
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
        return app
    }

    private func openSearch(_ app: XCUIApplication) {
        app.buttons["tab-search"].tap()
        XCTAssertTrue(app.textFields["search-field"].waitForExistence(timeout: 8))
    }

    private func shot(_ app: XCUIApplication, _ name: String) {
        let dir = URL(fileURLWithPath: "/tmp/mewo-browse-shots", isDirectory: true)
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
        return listening
    }

    private func setLibraryMode(_ mode: String) {
        var request = URLRequest(url: URL(string: "http://127.0.0.1:8766/control/library-search")!)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = Data(#"{"mode":"\#(mode)"}"#.utf8)
        let done = DispatchSemaphore(value: 0)
        URLSession.shared.dataTask(with: request) { _, _, _ in done.signal() }.resume()
        _ = done.wait(timeout: .now() + 2)
    }
}
