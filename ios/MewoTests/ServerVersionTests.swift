import XCTest
@testable import Mewo

final class ServerVersionTests: XCTestCase {
    func testDecodesVersionAndCommit() {
        let info = ServerVersion.decode(from: Data(#"{"version":"0.1.3","commit":"abcdef0"}"#.utf8))
        XCTAssertEqual(info, ServerVersion(version: "0.1.3", commit: "abcdef0"))
        XCTAssertEqual(info?.label, "0.1.3 (abcdef0)")
    }

    func testCommitNullMissingOrEmptyIsOmitted() {
        let nullCommit = ServerVersion.decode(from: Data(#"{"version":"0.1.3","commit":null}"#.utf8))
        XCTAssertEqual(nullCommit, ServerVersion(version: "0.1.3", commit: nil))
        XCTAssertEqual(nullCommit?.label, "0.1.3")

        let missing = ServerVersion.decode(from: Data(#"{"version":"0.1.3"}"#.utf8))
        XCTAssertEqual(missing?.commit, nil)

        let empty = ServerVersion.decode(from: Data(#"{"version":"0.1.3","commit":""}"#.utf8))
        XCTAssertEqual(empty?.commit, nil)
        XCTAssertEqual(empty?.label, "0.1.3")
    }

    func testNonStringCommitIsOmitted() {
        let info = ServerVersion.decode(from: Data(#"{"version":"0.1.3","commit":12}"#.utf8))
        XCTAssertEqual(info, ServerVersion(version: "0.1.3", commit: nil))
    }

    func testRejectsMissingOrEmptyVersion() {
        XCTAssertNil(ServerVersion.decode(from: Data(#"{"commit":"abcdef0"}"#.utf8)))
        XCTAssertNil(ServerVersion.decode(from: Data(#"{"version":""}"#.utf8)))
        XCTAssertNil(ServerVersion.decode(from: Data(#"{"version":null}"#.utf8)))
        XCTAssertNil(ServerVersion.decode(from: Data(#"{"version":1}"#.utf8)))
    }

    func testRejectsNonObjectPayloads() {
        XCTAssertNil(ServerVersion.decode(from: Data("[]".utf8)))
        XCTAssertNil(ServerVersion.decode(from: Data("null".utf8)))
        XCTAssertNil(ServerVersion.decode(from: Data()))
        XCTAssertNil(ServerVersion.decode(from: Data("not-json".utf8)))
    }

    func testIgnoresExtraFields() {
        let info = ServerVersion.decode(from: Data(#"{"version":"9.9.9","commit":"abc1234","extra":true}"#.utf8))
        XCTAssertEqual(info, ServerVersion(version: "9.9.9", commit: "abc1234"))
    }

    func testAppLabelIncludesBuildAndCommit() {
        XCTAssertEqual(AppBuild.format(version: "0.1.3", build: "2", commit: "fdf858c"), "0.1.3 (2 · fdf858c)")
        XCTAssertEqual(AppBuild.format(version: "0.1.3", build: "2", commit: nil), "0.1.3 (2)")
        XCTAssertEqual(AppBuild.format(version: "0.1.3", build: "2", commit: "  "), "0.1.3 (2)")
        XCTAssertEqual(AppBuild.format(version: "0.1.3", build: "", commit: nil), "0.1.3")
        XCTAssertEqual(AppBuild.format(version: "  ", build: "", commit: nil), "Unknown")
    }
}
