import XCTest

/// The Library form in each email state, in light and dark.
/// Shots land in /tmp/mewo-email-shots/<theme>/.
final class EmailScreenshotTests: XCTestCase {
    func testLightStates() throws {
        try walk(theme: "x_light")
    }

    func testDarkStates() throws {
        try walk(theme: "x_lights_out")
    }

    private func walk(theme: String) throws {
        let signIn = launch("sign-in", theme: theme, signedIn: false)
        XCTAssertTrue(signIn.buttons["hosted-forgot"].waitForExistence(timeout: 10))
        shot(theme, "sign-in")

        let reset = launch("reset", theme: theme, signedIn: false)
        XCTAssertTrue(reset.staticTexts["Reset password"].waitForExistence(timeout: 10))
        XCTAssertTrue(reset.textFields["hosted-login"].exists)
        XCTAssertTrue(reset.buttons["hosted-sign-in-instead"].exists)
        shot(theme, "reset")

        let create = launch("create", theme: theme, signedIn: false)
        XCTAssertTrue(create.textFields["hosted-email-field"].waitForExistence(timeout: 10))
        XCTAssertFalse(create.buttons["hosted-forgot"].exists)
        shot(theme, "create")

        let account = launch("account", theme: theme, signedIn: true)
        let row = account.buttons["hosted-email"]
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        XCTAssertTrue(row.label.contains("ada@example.com"), row.label)
        shot(theme, "account")
        row.tap()
        XCTAssertTrue(account.buttons["email-sheet-remove"].waitForExistence(timeout: 5))
        XCTAssertTrue(account.secureTextFields["email-sheet-password"].exists)
        // The sheet opens with the saved address, not an empty field.
        XCTAssertEqual(account.textFields["email-sheet-email"].value as? String, "ada@example.com")
        shot(theme, "email-sheet")
    }

    private func launch(_ state: String, theme: String, signedIn: Bool) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = [
            "--uitesting",
            "--hosted-url=http://127.0.0.1:9",
            "--hosted-user=ada",
            "--hosted-token=\(signedIn ? "token" : "")",
            "--theme=\(theme)",
            "--email-shots=\(state)",
        ]
        app.launch()
        XCTAssertTrue(app.descendants(matching: .any)["email-preview"].waitForExistence(timeout: 15))
        return app
    }

    private func shot(_ theme: String, _ name: String) {
        let dir = URL(fileURLWithPath: "/tmp/mewo-email-shots/\(theme)", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent("\(name).png")
        try? XCUIScreen.main.screenshot().pngRepresentation.write(to: file)
    }
}
