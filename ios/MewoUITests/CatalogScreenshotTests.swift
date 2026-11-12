import XCTest

/// Catalog tab and detail sheet against Fixtures/social-fixture.py on 127.0.0.1:8766.
/// The iOS job does not run UI tests. `project.yml` globs this folder; CI runs
/// `xcodegen generate` before MewoTests, and the committed xcodeproj can lag.
/// On a Mac:
///   cd ios && xcodegen generate && (python3 MewoUITests/Fixtures/social-fixture.py &) && xcodebuild test -project Mewo.xcodeproj -scheme Mewo -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:MewoUITests/CatalogScreenshotTests
/// Shots land in /tmp/mewo-catalog-shots.
final class CatalogScreenshotTests: XCTestCase {
    private static let slideToTypeCoachFinger = "sliding your finger"
    private static let slideToTypeCoachPrefix = "Speed up your typing"

    func testCatalogTrendingAndDetail() throws {
        guard fixtureReady() else {
            XCTFail("social fixture is not reachable on 127.0.0.1:8766")
            return
        }
        setFailSearch(false)
        let app = launchApp()

        app.buttons["tab-search"].tap()
        XCTAssertTrue(app.buttons["search-tab-catalog"].waitForExistence(timeout: 8))
        dismissKeyboard(app)
        app.buttons["search-tab-catalog"].tap()
        XCTAssertTrue(app.staticTexts["Trending on Open Library"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["Dune"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["Searching"].exists)
        XCTAssertTrue(app.staticTexts["The Left Hand of Darkness"].exists)
        reveal(app, "Downloading 42%")
        XCTAssertTrue(app.staticTexts["Downloading 42%"].exists)
        reveal(app, "On your shelf")
        XCTAssertTrue(app.staticTexts["On your shelf"].exists)
        reveal(app, "Failed: No release found.")
        XCTAssertTrue(app.staticTexts["Failed: No release found."].exists)
        reveal(app, "On server · @bea")
        XCTAssertTrue(app.staticTexts["On server · @bea"].exists)
        reveal(app, "Pale Fire")
        XCTAssertTrue(app.staticTexts["Pale Fire"].exists)
        reveal(app, "Parable of the Sower")
        XCTAssertTrue(app.staticTexts["Parable of the Sower"].exists)
        XCTAssertTrue(app.buttons["Request again"].exists)
        frameBoth(app, "On server · @bea", "Parable of the Sower")
        XCTAssertTrue(app.staticTexts["On server · @bea"].isHittable)
        XCTAssertTrue(app.staticTexts["Parable of the Sower"].isHittable)
        dismissKeyboard(app)
        shot(app, "catalog-trending-bottom")
        scrollToHeading(app)
        dismissKeyboard(app)
        shot(app, "catalog-trending")

        let field = app.textFields["search-field"]
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.tap()
        field.typeText("du")
        XCTAssertTrue(app.staticTexts["Trending on Open Library"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["The Left Hand of Darkness"].waitForExistence(timeout: 8))
        dismissKeyboard(app)
        shot(app, "catalog-short-query")

        setFailSearch(true)
        if app.buttons["search-clear"].exists {
            app.buttons["search-clear"].tap()
        }
        field.tap()
        field.typeText("dune")
        XCTAssertTrue(app.staticTexts["Couldn't search the catalog right now."].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["Try again"].exists)
        XCTAssertFalse(app.staticTexts["Trending on Open Library"].exists)
        dismissKeyboard(app)
        shot(app, "catalog-error")

        setFailSearch(false)
        app.buttons["catalog-feed-retry"].tap()
        XCTAssertTrue(app.staticTexts["Dune"].waitForExistence(timeout: 8))
        app.buttons["search-clear"].tap()
        XCTAssertTrue(app.staticTexts["Trending on Open Library"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["Dune"].waitForExistence(timeout: 8))
        dismissKeyboard(app)

        openRow(app, "OL1W")
        XCTAssertTrue(app.staticTexts["Looking for a release…"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["Cancel request"].exists)
        shot(app, "catalog-detail-searching")
        closeSheet(app)

        openRow(app, "OL3W")
        XCTAssertTrue(app.staticTexts["catalog-status"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["catalog-cancel"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["Downloading 42%"].exists)
        shot(app, "catalog-detail-downloading")
        closeSheet(app)

        openRow(app, "OL6W")
        XCTAssertTrue(app.staticTexts["catalog-status"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["catalog-retry"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["No release found."].exists)
        shot(app, "catalog-detail-failed")
        closeSheet(app)

        openRow(app, "OL4W")
        XCTAssertTrue(app.staticTexts["catalog-status"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["catalog-open"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["On your shelf"].exists)
        shot(app, "catalog-detail-on-shelf")
        closeSheet(app)

        openRow(app, "OL5W")
        XCTAssertTrue(app.buttons["catalog-again"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["Request again"].exists)
        XCTAssertFalse(app.buttons["Open book"].exists)
        XCTAssertFalse(app.buttons["catalog-open"].exists)
        XCTAssertFalse(app.staticTexts["catalog-status"].exists)
        shot(app, "catalog-detail-imported-no-book")
        closeSheet(app)

        openRow(app, "OL8W")
        XCTAssertTrue(app.buttons["catalog-again"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.buttons["Request again"].exists)
        XCTAssertFalse(app.buttons["Open book"].exists)
        XCTAssertFalse(app.staticTexts["catalog-status"].exists)
        shot(app, "catalog-detail-canceled")
        closeSheet(app)

        app.buttons["tab-home"].tap()
        XCTAssertTrue(app.buttons["home-books"].waitForExistence(timeout: 5))
        app.buttons["home-books"].tap()
        XCTAssertTrue(app.buttons["add-epub"].waitForExistence(timeout: 8))
        app.buttons["add-epub"].tap()
        let upload = app.buttons["Upload EPUB"]
        let browse = app.buttons["Browse catalog"]
        XCTAssertTrue(upload.waitForExistence(timeout: 5))
        XCTAssertTrue(browse.waitForExistence(timeout: 5))
        XCTAssertLessThan(upload.frame.minY, browse.frame.minY)
        shot(app, "add-book-menu")
    }

    func testCatalogGenreAndSortMenus() throws {
        guard fixtureReady() else {
            XCTFail("social fixture is not reachable on 127.0.0.1:8766")
            return
        }
        setFailSearch(false)
        let app = launchApp()

        app.buttons["tab-search"].tap()
        XCTAssertTrue(app.buttons["search-tab-catalog"].waitForExistence(timeout: 8))
        dismissKeyboard(app)
        app.buttons["search-tab-catalog"].tap()
        let genre = app.buttons["catalog-genre"]
        let sort = app.buttons["catalog-sort"]
        XCTAssertTrue(genre.waitForExistence(timeout: 8))
        XCTAssertTrue(sort.waitForExistence(timeout: 8))
        XCTAssertEqual(genre.label, "All genres")
        XCTAssertEqual(sort.label, "Popular")
        XCTAssertTrue(app.staticTexts["Trending on Open Library"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["Dune"].waitForExistence(timeout: 8))
        XCTAssertTrue(waitForCatalog(q: "", genre: nil, sort: nil))
        XCTAssertEqual(genre.frame.midY, sort.frame.midY, accuracy: 1)
        XCTAssertEqual(genre.frame.height, sort.frame.height, accuracy: 1)
        XCTAssertLessThan(genre.frame.maxY, app.staticTexts["catalog-heading"].frame.minY)
        dismissKeyboard(app)
        shot(app, "catalog-filters")

        genre.tap()
        let fantasy = app.buttons["Fantasy"]
        XCTAssertTrue(fantasy.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Poetry"].exists)
        shot(app, "catalog-genre-menu")
        fantasy.tap()
        XCTAssertTrue(waitForLabel(genre, "Fantasy"))
        XCTAssertTrue(waitForCatalog(q: "", genre: "fantasy", sort: nil))
        XCTAssertTrue(app.staticTexts["The Dispossessed"].waitForExistence(timeout: 8))
        XCTAssertFalse(app.staticTexts["Trending on Open Library"].exists)

        sort.tap()
        let newest = app.buttons["New"]
        XCTAssertTrue(newest.waitForExistence(timeout: 5))
        newest.tap()
        XCTAssertTrue(waitForLabel(sort, "New"))
        XCTAssertTrue(waitForCatalog(q: "", genre: "fantasy", sort: "new"))
        XCTAssertTrue(app.staticTexts["The Dispossessed"].waitForExistence(timeout: 8))
        XCTAssertFalse(app.staticTexts["Trending on Open Library"].exists)
        shot(app, "catalog-fantasy-new")

        let field = app.textFields["search-field"]
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.tap()
        field.typeText("dune")
        XCTAssertTrue(sort.waitForNonExistence(timeout: 5))
        XCTAssertEqual(genre.label, "Fantasy")
        XCTAssertTrue(waitForCatalog(q: "dune", genre: "fantasy", sort: nil))
        XCTAssertTrue(app.staticTexts["Dune"].waitForExistence(timeout: 8))
        dismissKeyboard(app)
        shot(app, "catalog-genre-search")

        app.buttons["search-clear"].tap()
        XCTAssertTrue(sort.waitForExistence(timeout: 5))
        XCTAssertEqual(sort.label, "New")
        dismissKeyboard(app)

        genre.tap()
        let poetry = app.buttons["Poetry"]
        XCTAssertTrue(poetry.waitForExistence(timeout: 5))
        poetry.tap()
        XCTAssertTrue(app.staticTexts["Nothing in Poetry right now. Try another genre."].waitForExistence(timeout: 8))
        XCTAssertEqual(genre.label, "Poetry")
        XCTAssertTrue(sort.exists)
        shot(app, "catalog-genre-empty")
    }

    func testSlideToTypeCoachLabelMatchesTheFreshSimulatorCopy() {
        XCTAssertTrue(slideToTypeCoachLabel("Speed up your typing by sliding your finger from letter to letter."))
        XCTAssertTrue(slideToTypeCoachLabel("Speed up your typing by sliding your finger across the letters to compose a word."))
        XCTAssertFalse(slideToTypeCoachLabel("Continue"))
        XCTAssertFalse(slideToTypeCoachLabel("Trending on Open Library"))
    }

    private func launchApp() -> XCUIApplication {
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

    private func waitForLabel(_ element: XCUIElement, _ label: String) -> Bool {
        let match = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", label), object: element)
        return XCTWaiter().wait(for: [match], timeout: 5) == .completed
    }

    /// The fixture records the last catalog search. Null genre or sort means it was not sent.
    private func waitForCatalog(q: String, genre: String?, sort: String?) -> Bool {
        let deadline = Date().addingTimeInterval(8)
        while Date() < deadline {
            if let last = controlJSON("/control/status")?["last_catalog"] as? [String: Any],
               (last["q"] as? String) == q,
               (last["genre"] as? String) == genre,
               (last["sort"] as? String) == sort {
                return true
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        return false
    }

    private func openRow(_ app: XCUIApplication, _ olid: String) {
        let row = app.buttons["catalog-\(olid)"]
        revealElement(app, row)
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        row.tap()
    }

    private func closeSheet(_ app: XCUIApplication) {
        app.buttons["Close"].tap()
        XCTAssertTrue(app.buttons["search-tab-catalog"].waitForExistence(timeout: 5))
    }

    private func reveal(_ app: XCUIApplication, _ label: String) {
        revealElement(app, app.staticTexts[label])
    }

    private func revealElement(_ app: XCUIApplication, _ element: XCUIElement) {
        for _ in 0..<8 {
            if element.exists { return }
            app.swipeUp()
        }
    }

    /// Slow nudges until the shared badge and the canceled title are both on screen.
    private func frameBoth(_ app: XCUIApplication, _ top: String, _ bottom: String) {
        let upper = app.staticTexts[top]
        let lower = app.staticTexts[bottom]
        for _ in 0..<12 {
            if upper.isHittable, lower.isHittable { return }
            if lower.exists, upper.exists, !upper.isHittable {
                app.swipeDown(velocity: .slow)
            } else if lower.exists, !upper.exists {
                app.swipeDown(velocity: .slow)
            } else {
                app.swipeUp(velocity: .slow)
            }
        }
    }

    private func scrollToHeading(_ app: XCUIApplication) {
        let heading = app.staticTexts["catalog-heading"]
        for _ in 0..<8 {
            if heading.exists, heading.frame.minY < 220 { return }
            app.swipeDown()
        }
    }

    private func shot(_ app: XCUIApplication, _ name: String) {
        assertNoTypingCoach(app)
        let dir = URL(fileURLWithPath: "/tmp/mewo-catalog-shots", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent("\(name).png")
        try? XCUIScreen.main.screenshot().pngRepresentation.write(to: file)
    }

    /// A fresh simulator covers the keyboard with the slide-to-type coach. Its
    /// Continue button is the control that removes it; the keys underneath have
    /// no hit point until then.
    private func dismissKeyboard(_ app: XCUIApplication, file: StaticString = #filePath, line: UInt = #line) {
        guard dismissTypingCoach(app, file: file, line: line) else { return }
        if app.keyboards.element.exists {
            let search = app.keyboards.buttons["Search"]
            let ret = app.keyboards.buttons["return"]
            if search.exists {
                tapKeyboardKey(search, app: app, file: file, line: line)
            } else if ret.exists {
                tapKeyboardKey(ret, app: app, file: file, line: line)
            }
        }
        assertNoTypingCoach(app, file: file, line: line)
    }

    private func dismissTypingCoach(_ app: XCUIApplication, file: StaticString = #filePath, line: UInt = #line) -> Bool {
        guard let button = typingCoachContinue(app) else { return true }
        button.tap()
        let intro = typingCoachView(app)
        let copy = slideToTypeCoach(app)
        let introGone = intro.exists ? intro.waitForNonExistence(timeout: 3) : true
        let copyGone = copy.exists ? copy.waitForNonExistence(timeout: 3) : true
        guard introGone, copyGone else {
            XCTFail("slide-to-type coach stayed on screen", file: file, line: line)
            return false
        }
        return true
    }

    private func typingCoachContinue(_ app: XCUIApplication) -> XCUIElement? {
        let intro = typingCoachView(app)
        if intro.exists {
            let nested = intro.buttons["Continue"]
            if nested.exists { return nested }
        }
        guard intro.exists || slideToTypeCoach(app).exists else { return nil }
        if app.buttons["Continue"].exists { return app.buttons["Continue"] }
        let onKeyboard = app.keyboards.buttons["Continue"]
        return onKeyboard.exists ? onKeyboard : nil
    }

    private func typingCoachView(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any)["UIContinuousPathIntroductionView"]
    }

    private func slideToTypeCoach(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any).matching(
            NSPredicate(
                format: "label CONTAINS %@ OR label BEGINSWITH %@",
                Self.slideToTypeCoachFinger,
                Self.slideToTypeCoachPrefix
            )
        ).firstMatch
    }

    private func slideToTypeCoachLabel(_ label: String) -> Bool {
        label.contains(Self.slideToTypeCoachFinger) || label.hasPrefix(Self.slideToTypeCoachPrefix)
    }

    private func assertNoTypingCoach(_ app: XCUIApplication, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertFalse(slideToTypeCoach(app).exists, "slide-to-type coach is covering the screen", file: file, line: line)
        XCTAssertFalse(typingCoachView(app).exists, "slide-to-type coach is covering the screen", file: file, line: line)
    }

    private func tapKeyboardKey(
        _ key: XCUIElement,
        app: XCUIApplication,
        file: StaticString,
        line: UInt
    ) {
        let deadline = Date().addingTimeInterval(2)
        while Date() < deadline, key.exists, !key.isHittable {
            if typingCoachContinue(app) != nil {
                guard dismissTypingCoach(app, file: file, line: line) else { return }
            }
            if key.isHittable { break }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        key.tap()
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

    private func setFailSearch(_ on: Bool) {
        guard let status = controlJSON("/control/status") else { return }
        if (status["fail_search"] as? Bool) == on { return }
        _ = controlJSON("/control/fail-search", post: true)
    }

    private func controlJSON(_ path: String, post: Bool = false) -> [String: Any]? {
        let url = URL(string: "http://127.0.0.1:8766\(path)")!
        var request = URLRequest(url: url)
        if post { request.httpMethod = "POST" }
        let done = DispatchSemaphore(value: 0)
        var payload: [String: Any]?
        URLSession.shared.dataTask(with: request) { data, _, _ in
            if let data {
                payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            }
            done.signal()
        }.resume()
        _ = done.wait(timeout: .now() + 2)
        return payload
    }
}
