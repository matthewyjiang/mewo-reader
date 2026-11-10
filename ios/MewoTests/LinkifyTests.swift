import UIKit
import XCTest
@testable import Mewo

@MainActor
final class LinkifyTests: XCTestCase {
    func testAUrlGluedToThePreviousWordIsALinkAndTrailingPunctuationIsNot() {
        let text = "supervision:https://en.wikipedia.org/wiki/Hopfield_network; https://en.wikipedia.org/wiki/Backpropagation."
        let spans = linkSpans(text)
        XCTAssertEqual(spans.map(\.url.absoluteString), [
            "https://en.wikipedia.org/wiki/Hopfield_network",
            "https://en.wikipedia.org/wiki/Backpropagation",
        ])
        XCTAssertEqual(String(text[spans[0].range]), spans[0].url.absoluteString)
        XCTAssertEqual(text[spans[0].range.upperBound], ";")
        XCTAssertEqual(text[spans[1].range.upperBound], ".")
    }

    func testATrailingSlashStaysAndTheSentencePeriodDoesNot() {
        let text = "microbes:https://www.ncbi.nlm.nih.gov/pmc/articles/PMC4790845/."
        let spans = linkSpans(text)
        XCTAssertEqual(spans.count, 1)
        XCTAssertEqual(text.distance(from: text.startIndex, to: spans[0].range.lowerBound), "microbes:".count)
        XCTAssertEqual(text.distance(from: text.startIndex, to: spans[0].range.upperBound), text.count - 1)
        XCTAssertEqual(spans[0].url.absoluteString, "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC4790845/")
    }

    func testProseAroundAUrlStaysOutsideTheLinkAndTheSlashStays() {
        let text = "go to https://www.ncbi.nlm.nih.gov/pmc/articles/ and at the search bar"
        let spans = linkSpans(text)
        XCTAssertEqual(spans.count, 1)
        XCTAssertEqual(spans[0].url.absoluteString, "https://www.ncbi.nlm.nih.gov/pmc/articles/")
        XCTAssertEqual(String(text[..<spans[0].range.lowerBound]), "go to ")
        XCTAssertEqual(String(text[spans[0].range.upperBound...]), " and at the search bar")
    }

    func testAnUnmatchedParenthesisInsideTheUrlIsKeptAndTheWrappingOneIsNot() {
        let text = "(https://example.com/wiki/Hopfield_network_(neural))."
        let spans = linkSpans(text)
        XCTAssertEqual(spans.map(\.url.absoluteString), [
            "https://example.com/wiki/Hopfield_network_(neural)",
        ])
        XCTAssertEqual(text[text.index(before: spans[0].range.lowerBound)], "(")
        XCTAssertEqual(String(text[spans[0].range.upperBound...]), ").")
    }

    func testALabelThatIsNotAUrlIsLeftAlone() {
        XCTAssertEqual(linkSpans("See this paper."), [])
    }

    func testAnIpv6UrlKeepsItsClosingBracket() {
        let text = "http://[::1]"
        let spans = linkSpans(text)
        XCTAssertEqual(spans.count, 1)
        XCTAssertEqual(spans[0].range, text.startIndex..<text.endIndex)
        XCTAssertEqual(spans[0].url.absoluteString, text)
        let dotted = "see http://[::1]."
        let dottedSpans = linkSpans(dotted)
        XCTAssertEqual(dottedSpans[0].url.absoluteString, "http://[::1]")
        XCTAssertEqual(dotted[dottedSpans[0].range.upperBound], ".")
        XCTAssertEqual(linkSpans("http://example.com/foo]").first?.url.absoluteString, "http://example.com/foo")
    }

    func testLinkedTextUnderlinesTheUrl() {
        let text = "go to https://www.ncbi.nlm.nih.gov/pmc/articles/ and at the search bar"
        let linked = linkedText(text, link: .blue)
        var urls: [String] = []
        for run in linked.runs {
            guard let url = run.link else { continue }
            urls.append(url.absoluteString)
            XCTAssertEqual(run.underlineStyle, .single)
            XCTAssertEqual(run.foregroundColor, .blue)
        }
        XCTAssertEqual(urls, ["https://www.ncbi.nlm.nih.gov/pmc/articles/"])
    }

    func testALinkTapOpensTheUrlAndDoesNotOpenTheRow() {
        let text = "see https://example.com/a today"
        let view = laidOut(text)
        var links: [String] = []
        var opens = 0
        view.onLink = { links.append($0.absoluteString) }
        view.onOpen = { opens += 1 }

        let span = linkSpans(text)[0]
        view.handleTap(at: viewPoint(in: view, range: span.range, text: text))

        XCTAssertEqual(links, ["https://example.com/a"])
        XCTAssertEqual(opens, 0)

        let see = text.range(of: "see")!
        view.handleTap(at: viewPoint(in: view, range: see, text: text))
        XCTAssertEqual(links, ["https://example.com/a"])
        XCTAssertEqual(opens, 1)
    }

    func testATapPastTheEndOfALinkOpensTheRow() {
        let text = "see https://example.com/a"
        let view = laidOut(text)
        var links: [String] = []
        var opens = 0
        view.onLink = { links.append($0.absoluteString) }
        view.onOpen = { opens += 1 }

        let used = view.layoutManager.usedRect(for: view.textContainer)
        XCTAssertLessThan(used.maxX, view.bounds.width - 8)
        view.handleTap(at: CGPoint(x: used.maxX + 8, y: used.midY))

        XCTAssertTrue(links.isEmpty)
        XCTAssertEqual(opens, 1)
    }

    private func laidOut(_ text: String) -> PostTextView {
        let view = PostTextView()
        view.bounds = CGRect(x: 0, y: 0, width: 320, height: 80)
        view.textContainer.size = CGSize(width: 320, height: CGFloat.greatestFiniteMagnitude)
        view.apply(
            text: text,
            font: .systemFont(ofSize: 17),
            lineSpacing: 7,
            ink: .label,
            link: .systemBlue
        )
        view.layoutManager.ensureLayout(for: view.textContainer)
        let height = view.sizeThatFits(CGSize(width: 320, height: CGFloat.greatestFiniteMagnitude)).height
        XCTAssertGreaterThan(height, 10)
        XCTAssertLessThan(height, 80)
        return view
    }

    private func viewPoint(in view: PostTextView, range: Range<String.Index>, text: String) -> CGPoint {
        let glyphs = view.layoutManager.glyphRange(
            forCharacterRange: NSRange(range, in: text),
            actualCharacterRange: nil
        )
        XCTAssertGreaterThan(glyphs.length, 0)
        let rect = view.layoutManager.boundingRect(
            forGlyphRange: NSRange(location: glyphs.location + glyphs.length / 2, length: 1),
            in: view.textContainer
        )
        return CGPoint(
            x: rect.midX + view.textContainerInset.left,
            y: rect.midY + view.textContainerInset.top
        )
    }
}
