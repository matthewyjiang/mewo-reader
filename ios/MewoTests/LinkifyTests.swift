import SwiftUI
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

    /// `makeUIView` builds `PostTextView(frame:textContainer:)`. `PostTextView()` is the other
    /// entry. Both have to disable editing and scrolling, clear the insets, install the tap,
    /// and stay on TextKit 1. Calling `handleTap` does not prove any of that.
    func testMakeUIViewConstructionConfiguresTheTextView() {
        assertConfigured(PostTextView(frame: .zero, textContainer: nil))
        assertConfigured(PostTextView())
    }

    private func assertConfigured(_ view: PostTextView, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertFalse(view.isEditable, file: file, line: line)
        XCTAssertFalse(view.isScrollEnabled, file: file, line: line)
        XCTAssertEqual(view.textContainerInset, .zero, file: file, line: line)
        XCTAssertEqual(view.textContainer.lineFragmentPadding, 0, file: file, line: line)
        XCTAssertEqual(view.backgroundColor, .clear, file: file, line: line)
        XCTAssertNil(view.textLayoutManager, "link hits use NSLayoutManager", file: file, line: line)
        XCTAssertNotNil(postTapRecognizer(on: view), file: file, line: line)
        _ = view.layoutManager
        XCTAssertNil(view.textLayoutManager, "reading the layout manager rebuilt the text view", file: file, line: line)
    }

    func testTheTextStorageOutlivesInit() {
        let text = "see https://example.com/a today"
        let view = autoreleasepool { () -> PostTextView in
            let view = PostTextView(frame: .zero, textContainer: nil)
            view.bounds = CGRect(x: 0, y: 0, width: 320, height: 80)
            view.apply(text: text, font: .systemFont(ofSize: 17), lineSpacing: 7, ink: .label, link: .systemBlue)
            return view
        }
        autoreleasepool {}

        let storage = view.textStorage
        let layout = view.layoutManager
        XCTAssertTrue(view.ownedTextStorage === storage)
        XCTAssertTrue(storage.layoutManagers.first === layout)
        XCTAssertTrue(layout.textContainers.first === view.textContainer)
        XCTAssertNil(view.textLayoutManager)

        view.layoutManager.ensureLayout(for: view.textContainer)
        let point = viewPoint(in: view, range: linkSpans(text)[0].range, text: text)
        XCTAssertEqual(view.link(at: point)?.absoluteString, "https://example.com/a")
        let see = text.range(of: "see")!
        XCTAssertNil(view.link(at: viewPoint(in: view, range: see, text: text)))
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

    func testARenderedFeedRowOpensALinkAndSizesToItsText() {
        let text = "see https://example.com/a today, then keep reading until this sentence wraps onto another line."
        let box = TapBox()
        let post = FeedPost(id: "p", kind: .body, text: text, chapter: "One")
        let root = ScrollView {
            LazyVStack(spacing: 0) {
                TimelinePost(
                    post: post,
                    author: "Ada",
                    handle: "ada",
                    cover: nil,
                    liked: false,
                    onComment: {},
                    onLike: {},
                    onRepost: {},
                    onQuote: {},
                    onShare: {},
                    onOpen: { box.opens += 1 }
                )
            }
        }
        .environment(\.mewo, DisplayTheme.xLight.palette)
        .environment(\.readerScale, .default)
        .environment(\.openURL, OpenURLAction { url in
            box.links.append(url.absoluteString)
            return .handled
        })
        .frame(width: 390, height: 844)
        .background(DisplayTheme.xLight.palette.ground)

        let window = show(root)
        defer { window.isHidden = true }
        guard let textView = findTextView(in: window) else {
            XCTFail("feed row did not build a PostTextView")
            return
        }

        XCTAssertFalse(textView.isEditable)
        XCTAssertFalse(textView.isScrollEnabled)
        XCTAssertEqual(textView.textContainerInset, .zero)
        XCTAssertEqual(textView.textContainer.lineFragmentPadding, 0)
        XCTAssertEqual(textView.backgroundColor, .clear)
        guard let recognizer = postTapRecognizer(on: textView) else {
            XCTFail("rendered text view has no tap recognizer")
            return
        }

        XCTAssertGreaterThan(textView.bounds.width, 200)
        XCTAssertGreaterThan(textView.bounds.height, textView.font?.lineHeight ?? 0)
        let used = textView.layoutManager.usedRect(for: textView.textContainer)
        XCTAssertGreaterThan(used.height, 10)
        XCTAssertLessThanOrEqual(used.maxY, textView.bounds.height + 1)

        let linkPoint = viewPoint(in: textView, range: linkSpans(text)[0].range, text: text)
        XCTAssertEqual(
            textView.link(at: linkPoint)?.absoluteString,
            "https://example.com/a",
            "bounds \(textView.bounds) point \(linkPoint) \(linkDump(textView))"
        )
        fire(recognizer, on: textView, at: linkPoint)
        XCTAssertEqual(box.links, ["https://example.com/a"])
        XCTAssertEqual(box.opens, 0)

        let see = text.range(of: "see")!
        fire(recognizer, on: textView, at: viewPoint(in: textView, range: see, text: text))
        XCTAssertEqual(box.links, ["https://example.com/a"])
        XCTAssertEqual(box.opens, 1)
    }

    func testCoderConstructionConfiguresTheTextView() throws {
        let data = try NSKeyedArchiver.archivedData(withRootObject: PostTextView(), requiringSecureCoding: false)
        let unarchiver = try NSKeyedUnarchiver(forReadingFrom: data)
        unarchiver.requiresSecureCoding = false
        let decoded = unarchiver.decodeObject(forKey: NSKeyedArchiveRootObjectKey) as? PostTextView
        let view = try XCTUnwrap(decoded)
        XCTAssertFalse(view.isEditable)
        XCTAssertFalse(view.isScrollEnabled)
        XCTAssertEqual(view.textContainerInset, .zero)
        XCTAssertEqual(view.textContainer.lineFragmentPadding, 0)
        XCTAssertEqual(view.backgroundColor, .clear)
        XCTAssertNotNil(postTapRecognizer(on: view))
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

    private final class TapBox {
        var links: [String] = []
        var opens = 0
    }

    private func show<V: View>(_ root: V) -> UIWindow {
        let host = UIHostingController(rootView: root)
        let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
        let window = scene.map { UIWindow(windowScene: $0) } ?? UIWindow(frame: CGRect(x: 0, y: 0, width: 390, height: 844))
        window.frame = CGRect(x: 0, y: 0, width: 390, height: 844)
        window.rootViewController = host
        window.makeKeyAndVisible()
        host.view.frame = window.bounds
        host.view.setNeedsLayout()
        host.view.layoutIfNeeded()
        let laidOut = expectation(description: "row laid out")
        DispatchQueue.main.async { laidOut.fulfill() }
        wait(for: [laidOut], timeout: 2)
        host.view.layoutIfNeeded()
        return window
    }

    private func findTextView(in view: UIView) -> PostTextView? {
        if let text = view as? PostTextView { return text }
        for subview in view.subviews {
            if let found = findTextView(in: subview) { return found }
        }
        return nil
    }

    /// Runs the recognizer installed on the row. The point is the one `tapped` reads back.
    private func fire(_ recognizer: PostTapRecognizer, on view: PostTextView, at point: CGPoint) {
        XCTAssertTrue(recognizer.view === view)
        recognizer.injectedLocation = point
        XCTAssertEqual(recognizer.location(in: view), point)
        view.perform(NSSelectorFromString("tapped:"), with: recognizer)
    }

    private func linkDump(_ view: PostTextView) -> String {
        guard let attributed = view.attributedText, attributed.length > 0 else { return "no text" }
        var parts: [String] = []
        attributed.enumerateAttribute(.link, in: NSRange(location: 0, length: attributed.length)) { value, range, _ in
            guard value != nil else { return }
            parts.append("\(range.location)+\(range.length)=\(value!)")
        }
        return parts.joined(separator: ", ")
    }

    private func postTapRecognizer(on view: PostTextView) -> PostTapRecognizer? {
        view.gestureRecognizers?.compactMap { $0 as? PostTapRecognizer }.first
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
