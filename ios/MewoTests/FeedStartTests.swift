import XCTest
@testable import Mewo

final class FeedStartTests: XCTestCase {
    func testSkipsPublisherChrome() {
        XCTAssertTrue(isPublisherChromeTitle("Copyright"))
        XCTAssertTrue(isPublisherChromeTitle("Table of Contents"))
        XCTAssertTrue(isPublisherChromeTitle("Also by this author"))
        XCTAssertTrue(isPublisherChromeTitle("About the Publisher"))
        XCTAssertFalse(isPublisherChromeTitle("Chapter 1"))
        XCTAssertFalse(isPublisherChromeTitle("Prologue"))
        XCTAssertFalse(isPublisherChromeTitle("A Modest Proposal"))
    }

    func testFeedStartsAtFirstNonChromeTOC() {
        let toc: [(key: String, value: String)] = [
            ("cover.xhtml", "Cover"),
            ("copyright.xhtml", "Copyright"),
            ("contents.xhtml", "Contents"),
            ("ch01.xhtml", "Chapter 1"),
            ("ch02.xhtml", "Chapter 2"),
        ]
        XCTAssertEqual(feedStartHref(tocByHref: toc), "ch01.xhtml")
    }

    func testHrefKeyStripsFragmentAndPath() {
        XCTAssertEqual(hrefKey("OEBPS/Text/ch01.xhtml#foo"), "ch01.xhtml")
        XCTAssertEqual(hrefKey("/Text/ch01.xhtml?q=1"), "ch01.xhtml")
    }

    func testSameHeadingIgnoresChapterPrefix() {
        XCTAssertTrue(sameHeading("One", tocTitle: "Chapter One"))
        XCTAssertTrue(sameHeading("Chapter One", tocTitle: "Chapter One"))
        XCTAssertFalse(sameHeading("Something else", tocTitle: "Chapter One"))
    }

    func testHandleRules() {
        XCTAssertFalse(LocalIdentity.parse(name: "", handle: "ab").isSuccess)
        XCTAssertTrue(LocalIdentity.parse(name: "Matt", handle: "matt").isSuccess)
        XCTAssertEqual(LocalIdentity.normalizeHandle("@matt"), "matt")
        XCTAssertNotNil(LocalIdentity.parse(name: "", handle: "bad-name").error)
    }
}
