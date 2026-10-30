import XCTest

/// Shelf rows in the four download states, and the Settings row in its three states.
final class OfflineScreenshotTests: XCTestCase {
    func testShelfAndSettingsStates() throws {
        let app = XCUIApplication()
        app.launchArguments = ["--uitesting", "--offline-shots"]
        app.launch()

        let preview = app.descendants(matching: .any)["offline-preview"]
        XCTAssertTrue(preview.waitForExistence(timeout: 15))
        let shelf = app.otherElements["offline-shelf"]
        XCTAssertTrue(shelf.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Download"].exists)
        XCTAssertTrue(app.buttons["Downloading, 40%"].exists)
        XCTAssertTrue(app.buttons["Remove download"].exists)
        XCTAssertTrue(app.buttons["Download failed, try again"].exists)
        XCTAssertTrue(app.staticTexts["120 posts · 34%"].exists)
        XCTAssertTrue(app.staticTexts["Shared by @ada"].exists)

        let settings = app.otherElements["offline-settings"]
        if !settings.isHittable {
            app.swipeUp()
        }
        XCTAssertTrue(settings.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Download all books"].exists)
        XCTAssertTrue(app.buttons["Downloading 3 of 12"].exists)
        XCTAssertTrue(app.staticTexts["All books downloaded"].exists)
        XCTAssertFalse(app.buttons["All books downloaded"].exists)

        let dir = URL(fileURLWithPath: "/tmp/mewo-offline-shots", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try shelf.screenshot().pngRepresentation.write(to: dir.appendingPathComponent("shelf.png"))
        try settings.screenshot().pngRepresentation.write(to: dir.appendingPathComponent("settings.png"))
        try XCUIScreen.main.screenshot().pngRepresentation.write(to: dir.appendingPathComponent("screen.png"))
    }
}
