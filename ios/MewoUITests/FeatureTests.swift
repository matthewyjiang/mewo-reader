import XCTest

/// Drives every product path in PRODUCT.md on a fresh Simulator install.
final class FeatureTests: XCTestCase {
    var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.terminate()
        app.launchArguments = ["--uitesting"]
        app.launch()
    }

    func testOnboardingRejectsShortHandle() {
        XCTAssertTrue(app.staticTexts["This is you"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["Next"].isEnabled)
        typeInto(field("handle-field"), "ab")
        XCTAssertFalse(app.buttons["Next"].isEnabled)
        XCTAssertTrue(app.staticTexts["Handle must be 3 to 32 characters (asked 2)."].waitForExistence(timeout: 3))
    }

    func testOnboardingNameIsOptionalThenEmptyLibrary() {
        onboard(name: "", handle: "matt")
        XCTAssertTrue(app.buttons["open-sample"].exists)
        XCTAssertTrue(app.buttons["Add a book"].exists)
        XCTAssertTrue(app.buttons["tab-home"].exists)
        XCTAssertTrue(app.buttons["tab-search"].exists)
        XCTAssertTrue(app.buttons["tab-likes"].exists)
    }

    func testImportSampleAndReadFeed() {
        onboard()
        openSample()
        openSampleBook()
        XCTAssertTrue(firstLine.waitForExistence(timeout: 15))
        XCTAssertTrue(app.staticTexts["The first post"].exists)
        XCTAssertFalse(app.staticTexts["Copyright"].exists)
    }

    func testLikePersistsOnLikesTab() {
        onboard()
        openSample()
        openSampleBook()
        likeFirstBody()
        app.buttons["Back"].firstMatch.tap()
        app.buttons["tab-likes"].tap()
        XCTAssertTrue(app.buttons["Unlike"].waitForExistence(timeout: 10))
    }

    func testCommentShowsOnOwnProfile() {
        onboard(name: "Matt", handle: "matt")
        openSample()
        openSampleBook()
        commentOnFirstBody("this line stayed with me")
        if app.buttons["Close"].waitForExistence(timeout: 3) {
            app.buttons["Close"].tap()
        }
        app.buttons["Back"].firstMatch.tap()
        openAccount()
        tapCenter("drawer-profile")
        XCTAssertTrue(waitAny(8, [
            app.staticTexts["1 post"],
            app.staticTexts["0 posts"],
            app.staticTexts["Reply to a line. It shows up here."]
        ]), "profile did not open")
        XCTAssertTrue(app.staticTexts["this line stayed with me"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["1 post"].exists)
    }

    func testSearchFindsBookAndPost() {
        onboard()
        openSample()
        app.buttons["tab-search"].tap()
        let search = field("search-field")
        typeInto(search, "windowsill")
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "windowsill")).firstMatch.waitForExistence(timeout: 10))
        search.tap()
        search.clearAndType("Timeline of My Own")
        XCTAssertTrue(app.staticTexts["A Timeline of My Own"].waitForExistence(timeout: 10))
    }

    func testChapterJump() {
        onboard()
        openSample()
        openSampleBook()
        app.buttons["Chapters"].tap()
        XCTAssertTrue(app.staticTexts["A paragraph that refused to split"].waitForExistence(timeout: 5))
        tapLabel("Caught up")
        XCTAssertTrue(app.staticTexts["I used to end with a blank leaf and a printer's ornament. Now I end when you run out of posts."].waitForExistence(timeout: 8))
    }

    func testQuoteAndShareSheets() {
        onboard()
        openSample()
        openSampleBook()
        tapPostAction("Quote this line")
        XCTAssertTrue(shareVisible)
        dismissShare()
        tapPostAction("Share")
        XCTAssertTrue(shareVisible)
        dismissShare()
    }

    func testFilePickerOpensFromEmptyLibrary() {
        onboard()
        app.buttons["Add a book"].tap()
        XCTAssertTrue(
            app.buttons["Cancel"].waitForExistence(timeout: 8) || app.buttons["Browse"].waitForExistence(timeout: 2)
        )
        if app.buttons["Cancel"].exists { app.buttons["Cancel"].tap() }
    }

    func testRemoveBookReturnsToEmptyLibrary() {
        onboard()
        openSample()
        let remove = app.buttons["Remove A Timeline of My Own"]
        XCTAssertTrue(remove.waitForExistence(timeout: 8))
        remove.tap()
        XCTAssertTrue(app.buttons["Remove"].waitForExistence(timeout: 5))
        app.buttons["Remove"].tap()
        XCTAssertTrue(app.staticTexts["Your timeline is empty"].waitForExistence(timeout: 8))
    }

    func testChangeHandleInSettings() {
        onboard(name: "Matt", handle: "matt")
        openSettings()
        tapSettings("account")
        XCTAssertTrue(app.buttons["Save"].waitForExistence(timeout: 5), "account page did not open")
        let handle = field("handle-field")
        handle.clearAndType("matthew")
        dismissKeyboard()
        app.buttons["Save"].tap()
        app.buttons["Back"].firstMatch.tap()
        app.buttons["Back"].firstMatch.tap()
        openAccount()
        XCTAssertTrue(app.staticTexts["@matthew"].waitForExistence(timeout: 5))
    }

    func testDisplayThemesFontAndMewoMode() {
        onboard()
        openSettings()
        tapSettings("display")
        XCTAssertTrue(waitAny(6, [
            app.staticTexts["Font size"],
            app.buttons["Twitter light"],
            app.buttons["theme-twitter_light"]
        ]), "display page did not open")
        tapLabel("Twitter light")
        tapLabel("Twitter dim")
        tapLabel("X light")
        tapLabel("Lights out")
        if app.buttons["Larger text"].exists { app.buttons["Larger text"].tap() }
        if app.buttons["Smaller text"].exists { app.buttons["Smaller text"].tap() }
        app.swipeUp()
        let toggle = firstExisting([
            app.switches["mewo-mode"],
            app.switches["Mewo mode"],
            app.buttons["mewo-mode"],
            app.buttons["Mewo mode"],
            app.otherElements["mewo-mode"]
        ], timeout: 5)
        XCTAssertNotNil(toggle, "Mewo mode toggle missing")
        toggle?.tap()
    }

    func testLibraryHostedForm() {
        onboard()
        openSettings()
        tapSettings("library")
        XCTAssertTrue(app.buttons["backend-hosted"].waitForExistence(timeout: 5) || app.buttons["Hosted server"].waitForExistence(timeout: 5))
        tapHosted()
        XCTAssertTrue(field("server-url").waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Sign in"].exists)
        XCTAssertTrue(app.buttons["Create account"].exists)
        tapLocal()
        XCTAssertFalse(app.buttons["Create account"].exists)
    }

    func testHostedCreateAccountImportSampleAndLike() {
        onboard(name: "Host", handle: "hostuser")
        openSettings()
        tapSettings("library")
        tapHosted()
        let url = field("server-url")
        url.clearAndType("http://127.0.0.1:8787")
        typeInto(field("hosted-username"), "simuser")
        typeInto(field("hosted-password"), "simpass12")
        dismissKeyboard()
        app.buttons["Create account"].tap()
        let signedIn = [app.buttons["hosted-sign-out"], app.buttons["Sign out"]]
        if firstExisting(signedIn, timeout: 8) == nil, app.buttons["Sign in"].exists {
            app.buttons["Sign in"].tap()
        }
        XCTAssertTrue(waitAny(15, signedIn), "hosted account did not sign in")
        tapCenter("Back")
        tapCenter("Back")
        if app.buttons["open-sample"].waitForExistence(timeout: 8) {
            openSample()
        }
        XCTAssertTrue(app.staticTexts["A Timeline of My Own"].waitForExistence(timeout: 20), "hosted library empty")
        app.staticTexts["A Timeline of My Own"].tap()
        XCTAssertTrue(app.buttons["Chapters"].waitForExistence(timeout: 20))
        likeFirstBody()
        app.buttons["Back"].firstMatch.tap()
        app.buttons["tab-likes"].tap()
        XCTAssertTrue(app.buttons["Unlike"].waitForExistence(timeout: 10))
    }

    func testChromeHidesOnScrollDown() {
        onboard()
        openSample()
        openSampleBook()
        let feed = app.scrollViews.firstMatch
        XCTAssertTrue(feed.waitForExistence(timeout: 8))
        feed.swipeUp()
        feed.swipeUp()
        if app.buttons["tab-home"].exists {
            XCTAssertFalse(app.buttons["tab-home"].isHittable)
        }
        feed.swipeDown()
        feed.swipeDown()
        XCTAssertTrue(app.buttons["Chapters"].waitForExistence(timeout: 5))
    }

    func testReadingPlaceIsRemembered() {
        onboard()
        openSample()
        openSampleBook()
        let feed = app.scrollViews.firstMatch
        for _ in 0..<8 { feed.swipeUp() }
        XCTAssertTrue(
            app.staticTexts["End of A Timeline of My Own"].waitForExistence(timeout: 8)
            || app.staticTexts["Caught up"].exists
        )
        app.buttons["Back"].firstMatch.tap()
        openSampleBook()
        XCTAssertTrue(
            app.staticTexts["I used to end with a blank leaf and a printer's ornament. Now I end when you run out of posts."].waitForExistence(timeout: 10)
            || app.staticTexts["End of A Timeline of My Own"].exists
        )
    }

    private var firstLine: XCUIElement {
        app.staticTexts["I did not mean to become a timeline. I meant to be a book. Someone left me on a phone, and the pages forgot how to turn."]
    }

    private var shareVisible: Bool {
        app.buttons["Copy"].waitForExistence(timeout: 4)
            || app.buttons["Close"].exists
            || app.otherElements["ActivityListView"].exists
    }

    private func onboard(name: String = "Matt", handle: String = "matt") {
        XCTAssertTrue(app.staticTexts["This is you"].waitForExistence(timeout: 10))
        if !name.isEmpty {
            typeInto(field("name-field"), name)
        }
        typeInto(field("handle-field"), handle)
        dismissKeyboard()
        let next = app.buttons["Next"]
        XCTAssertTrue(next.waitForExistence(timeout: 3))
        XCTAssertTrue(next.isEnabled)
        next.tap()
        XCTAssertTrue(app.buttons["open-sample"].waitForExistence(timeout: 10) || app.buttons["tab-home"].waitForExistence(timeout: 3))
    }

    private func openSample() {
        let sample = app.buttons["open-sample"]
        if sample.waitForExistence(timeout: 4) {
            sample.tap()
        }
        XCTAssertTrue(app.staticTexts["A Timeline of My Own"].waitForExistence(timeout: 20))
    }

    private func openSampleBook() {
        let title = app.staticTexts["A Timeline of My Own"]
        XCTAssertTrue(title.waitForExistence(timeout: 8))
        title.tap()
        XCTAssertTrue(app.buttons["Chapters"].waitForExistence(timeout: 20))
    }

    private func likeFirstBody() {
        tapPostAction("Like")
        XCTAssertTrue(app.buttons["Unlike"].waitForExistence(timeout: 5))
    }

    private func commentOnFirstBody(_ text: String) {
        tapPostAction("Comment on this line")
        let reply = field("reply-field")
        typeInto(reply, text)
        dismissKeyboard()
        app.buttons["Reply"].tap()
        XCTAssertTrue(app.staticTexts[text].waitForExistence(timeout: 8))
    }

    private func tapPostAction(_ label: String) {
        let button = app.buttons[label].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 10))
        button.tap()
    }

    private func openAccount() {
        let account = app.buttons["account-button"]
        XCTAssertTrue(account.waitForExistence(timeout: 5))
        account.tap()
        XCTAssertTrue(
            app.buttons["drawer-settings"].waitForExistence(timeout: 5)
            || app.buttons["Settings"].waitForExistence(timeout: 2)
        )
    }

    private func openSettings() {
        openAccount()
        tapCenter("drawer-settings")
        let opened = waitAny(6, [
            app.otherElements["settings-home"],
            app.staticTexts["settings-home"],
            app.buttons["settings-display"],
            app.buttons["settings-account"]
        ])
        if !opened {
            let dump = app.debugDescription
            try? dump.write(toFile: "/tmp/mewo-hierarchy-settings.txt", atomically: true, encoding: .utf8)
        }
        XCTAssertTrue(opened, "settings did not open after drawer-settings tap")
    }

    private func tapSettings(_ name: String) {
        tapCenter("settings-\(name)")
    }

    private func tapHosted() {
        if app.buttons["backend-hosted"].waitForExistence(timeout: 3) {
            app.buttons["backend-hosted"].tap()
        } else {
            tapLabel("Hosted server")
        }
    }

    private func tapLocal() {
        if app.buttons["backend-local"].waitForExistence(timeout: 3) {
            app.buttons["backend-local"].tap()
        } else {
            tapLabel("On this phone")
        }
    }

    private func waitAny(_ timeout: TimeInterval, _ elements: [XCUIElement]) -> Bool {
        firstExisting(elements, timeout: timeout) != nil
    }

    private func firstExisting(_ elements: [XCUIElement], timeout: TimeInterval) -> XCUIElement? {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let match = elements.first(where: \.exists) { return match }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        return elements.first(where: \.exists)
    }

    private func tapCenter(_ id: String) {
        let button = app.buttons[id]
        XCTAssertTrue(button.waitForExistence(timeout: 5), "missing \(id)")
        button.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
    }

    private func tapHittable(_ names: [String]) {
        for name in names {
            let button = app.buttons[name].firstMatch
            guard button.waitForExistence(timeout: 3) else { continue }
            guard button.isHittable else { continue }
            button.tap()
            return
        }
        XCTFail("No hittable button among \(names)")
    }

    private func tapLabel(_ label: String) {
        let button = app.buttons[label].firstMatch
        if button.waitForExistence(timeout: 3) {
            button.tap()
            return
        }
        let text = app.staticTexts[label].firstMatch
        XCTAssertTrue(text.waitForExistence(timeout: 5))
        text.tap()
    }

    private func field(_ id: String) -> XCUIElement {
        let box = app.textFields[id]
        if box.waitForExistence(timeout: 3) { return box }
        let secure = app.secureTextFields[id]
        if secure.waitForExistence(timeout: 1) { return secure }
        switch id {
        case "name-field":
            return app.textFields.element(boundBy: 0)
        case "handle-field":
            return app.textFields.count > 1 ? app.textFields.element(boundBy: 1) : app.textFields.element(boundBy: 0)
        case "hosted-username":
            return app.textFields.element(boundBy: app.textFields.count > 1 ? 1 : 0)
        case "hosted-password":
            return app.secureTextFields.firstMatch
        case "server-url":
            return app.textFields.firstMatch
        case "search-field":
            return app.textFields.firstMatch
        case "reply-field":
            return app.textFields.firstMatch
        default:
            return box
        }
    }

    private func typeInto(_ element: XCUIElement, _ text: String) {
        XCTAssertTrue(element.waitForExistence(timeout: 5))
        element.tap()
        element.typeText(text)
    }

    private func dismissKeyboard() {
        if !app.keyboards.element.exists { return }
        if app.keyboards.buttons["return"].exists {
            app.keyboards.buttons["return"].tap()
        } else if app.buttons["Done"].exists {
            app.buttons["Done"].tap()
        } else {
            app.staticTexts["This is you"].tap()
        }
    }

    private func dismissShare() {
        if app.buttons["Close"].exists {
            app.buttons["Close"].tap()
        } else if app.buttons["Cancel"].exists {
            app.buttons["Cancel"].tap()
        } else {
            app.swipeDown()
        }
    }
}

private extension XCUIElement {
    func clearAndType(_ text: String) {
        tap()
        guard let current = value as? String, !current.isEmpty else {
            typeText(text)
            return
        }
        let delete = String(repeating: XCUIKeyboardKey.delete.rawValue, count: current.count)
        typeText(delete)
        typeText(text)
    }
}
