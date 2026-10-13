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

    func testFinishedBooksLeaveTheTimeline() {
        func book(_ id: String, posts: Int, index: Int) -> BookRecord {
            BookRecord(
                id: id, title: id, author: "Author", handle: "author",
                importedAt: 1, postCount: posts, progressIndex: index
            )
        }
        let unread = book("unread", posts: 0, index: 0)
        let reading = book("reading", posts: 10, index: 3)
        let atEnd = book("end", posts: 10, index: 9)
        let onePost = book("one", posts: 1, index: 0)
        let onePostDone = book("done", posts: 1, index: 1)
        XCTAssertFalse(unread.isFinished)
        XCTAssertFalse(reading.isFinished)
        XCTAssertTrue(atEnd.isFinished)
        XCTAssertFalse(onePost.isFinished)
        XCTAssertTrue(onePostDone.isFinished)
        XCTAssertEqual(statusLine(for: unread), "Not opened yet")
        XCTAssertEqual(statusLine(for: reading), "10 posts · 40%")
        XCTAssertEqual(statusLine(for: atEnd), "10 posts · Finished")
    }

    func testHandleRules() {
        XCTAssertFalse(LocalIdentity.parse(name: "", handle: "ab").isSuccess)
        XCTAssertTrue(LocalIdentity.parse(name: "Matt", handle: "matt").isSuccess)
        XCTAssertEqual(LocalIdentity.normalizeHandle("@matt"), "matt")
        XCTAssertNotNil(LocalIdentity.parse(name: "", handle: "bad-name").error)
    }
}
