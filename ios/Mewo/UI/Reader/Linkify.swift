import SwiftUI
import UIKit

/// A URL span in post text. `range` is in `String` indices, which for these
/// ASCII matches line up with the UTF-16 indexes web and Android use.
struct LinkSpan: Equatable {
    var range: Range<String.Index>
    var url: URL
}

private let trailingPunctuation = Set<Character>(Array(".,;:!?)]}'\""))

/// Link spans for post text. Same rule as web `linkSpans`.
///
/// Find the earliest `http://` or `https://` from the cursor, including one
/// sitting against the previous word (`supervision:https://…`). End at the
/// first Unicode whitespace character. Then strip trailing `.,;:!?)]}'"`.
/// Keep a `)` or `]` when the URL before that closer has more of its opener
/// than its closer, so `http://[::1]` keeps the bracket and an inner `)` in
/// `Hopfield_network_(neural)` stays. Keep a trailing `/`. A match that does
/// not extend past the scheme is not a link. Matching is case-sensitive and
/// does not require a word boundary.
func linkSpans(_ text: String) -> [LinkSpan] {
    let https = "https://"
    let http = "http://"
    var spans: [LinkSpan] = []
    var cursor = text.startIndex
    while cursor < text.endIndex {
        let rest = cursor..<text.endIndex
        let httpsAt = text.range(of: https, range: rest)?.lowerBound
        let httpAt = text.range(of: http, range: rest)?.lowerBound
        let start: String.Index
        switch (httpsAt, httpAt) {
        case let (httpsAt?, httpAt?):
            start = min(httpsAt, httpAt)
        case let (httpsAt?, nil):
            start = httpsAt
        case let (nil, httpAt?):
            start = httpAt
        case (nil, nil):
            return spans
        }
        let scheme = text[start...].hasPrefix(https) ? https : http
        let schemeEnd = text.index(start, offsetBy: scheme.count)
        var end = start
        while end < text.endIndex, !text[end].isWhitespace {
            end = text.index(after: end)
        }
        while end > schemeEnd {
            let previous = text.index(before: end)
            let character = text[previous]
            guard trailingPunctuation.contains(character) else { break }
            if keepCloser(character, text, from: start, to: previous) { break }
            end = previous
        }
        if end > schemeEnd, let url = URL(string: String(text[start..<end])) {
            spans.append(LinkSpan(range: start..<end, url: url))
            cursor = end
        } else {
            cursor = schemeEnd
        }
    }
    return spans
}

/// Post text with `.link` on each span, drawn in `link` and underlined.
/// Web uses the accent color and an underline; Android uses the same.
func linkedText(_ text: String, link: Color) -> AttributedString {
    let spans = linkSpans(text)
    guard !spans.isEmpty else { return AttributedString(text) }
    var result = AttributedString()
    var cursor = text.startIndex
    for span in spans {
        if cursor < span.range.lowerBound {
            result += AttributedString(String(text[cursor..<span.range.lowerBound]))
        }
        var piece = AttributedString(String(text[span.range]))
        piece.link = span.url
        piece.foregroundColor = link
        piece.underlineStyle = .single
        result += piece
        cursor = span.range.upperBound
    }
    if cursor < text.endIndex {
        result += AttributedString(String(text[cursor..<text.endIndex]))
    }
    return result
}

private func keepCloser(
    _ closer: Character,
    _ text: String,
    from start: String.Index,
    to end: String.Index
) -> Bool {
    switch closer {
    case ")": return unmatched(text, from: start, to: end, open: "(", close: ")")
    case "]": return unmatched(text, from: start, to: end, open: "[", close: "]")
    default: return false
    }
}

private func unmatched(
    _ text: String,
    from start: String.Index,
    to end: String.Index,
    open: Character,
    close: Character
) -> Bool {
    var opened = 0
    var closed = 0
    var index = start
    while index < end {
        let character = text[index]
        if character == open { opened += 1 }
        else if character == close { closed += 1 }
        index = text.index(after: index)
    }
    return opened > closed
}

/// The post body. A tap on a link opens that URL and does not call `onOpen`.
/// A tap on the rest of the text calls `onOpen` when the row opens a book.
struct LinkedPostText: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.openURL) private var openURL
    @Environment(\.readerScale) private var scale
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    var text: String
    var heading: Bool = false
    /// Extra space between lines. `nil` uses the reader scale, matching `Text` line spacing.
    var lineSpacing: CGFloat? = nil
    var onOpen: (() -> Void)?

    var body: some View {
        let size = heading ? scale.headingSize : scale.bodySize
        let spacing = lineSpacing ?? max(0, (heading ? scale.headingLine : scale.bodyLine) - size)
        let name = heading ? MewoFont.bold : MewoFont.regular
        let base = UIFont(name: name, size: size) ?? .systemFont(ofSize: size, weight: heading ? .bold : .regular)
        let font = UIFontMetrics(forTextStyle: heading ? .headline : .body).scaledFont(
            for: base,
            compatibleWith: UITraitCollection(preferredContentSizeCategory: dynamicTypeSize.uiContentSize)
        )
        PostTextRepresentable(
            text: text,
            font: font,
            lineSpacing: spacing,
            ink: UIColor(mewo.ink),
            link: UIColor(mewo.accent),
            onOpen: onOpen,
            onLink: { openURL($0) }
        )
        .fixedSize(horizontal: false, vertical: true)
    }
}

private extension DynamicTypeSize {
    var uiContentSize: UIContentSizeCategory {
        switch self {
        case .xSmall: return .extraSmall
        case .small: return .small
        case .medium: return .medium
        case .large: return .large
        case .xLarge: return .extraLarge
        case .xxLarge: return .extraExtraLarge
        case .xxxLarge: return .extraExtraExtraLarge
        case .accessibility1: return .accessibilityMedium
        case .accessibility2: return .accessibilityLarge
        case .accessibility3: return .accessibilityExtraLarge
        case .accessibility4: return .accessibilityExtraExtraLarge
        case .accessibility5: return .accessibilityExtraExtraExtraLarge
        @unknown default: return .large
        }
    }
}

/// Draws the linked post and owns the tap, so a row `onTapGesture` is not wrapped around it.
final class PostTextView: UITextView {
    var onOpen: (() -> Void)?
    var onLink: ((URL) -> Void)?
    private var renderedKey: String?
    /// `NSTextContainer.layoutManager` does not retain the storage. This property does,
    /// from before `super.init` until the view is released. A local tuple is not enough:
    /// an optimized build can drop it once `init` has read the container.
    let ownedTextStorage: NSTextStorage

    /// `PostTextView()` and `makeUIView` both land here. A nil container would select
    /// TextKit 2, and the first `layoutManager` read (sizing and link hits) would then
    /// rebuild the view in TextKit 1 compatibility mode. Link hit testing uses that
    /// layout manager, so a nil container is wired to TextKit 1 before `super`.
    override init(frame: CGRect, textContainer: NSTextContainer?) {
        let built = Self.textKit1Stack(textContainer)
        ownedTextStorage = built.storage
        super.init(frame: frame, textContainer: built.container)
        configure()
    }

    required init?(coder: NSCoder) {
        // A nib builds and retains its own stack.
        ownedTextStorage = NSTextStorage()
        super.init(coder: coder)
        configure()
    }

    private struct TextKit1Stack {
        var storage: NSTextStorage
        var container: NSTextContainer
    }

    /// The returned storage is assigned to `ownedTextStorage` before `super.init`.
    private static func textKit1Stack(_ textContainer: NSTextContainer?) -> TextKit1Stack {
        if let textContainer, let storage = textContainer.layoutManager?.textStorage {
            return TextKit1Stack(storage: storage, container: textContainer)
        }
        if let textContainer, textContainer.textLayoutManager != nil {
            return TextKit1Stack(storage: NSTextStorage(), container: textContainer)
        }
        let storage = NSTextStorage()
        let layout = NSLayoutManager()
        let container = textContainer ?? NSTextContainer()
        storage.addLayoutManager(layout)
        layout.addTextContainer(container)
        return TextKit1Stack(storage: storage, container: container)
    }

    private func configure() {
        isEditable = false
        isSelectable = false
        isScrollEnabled = false
        isAccessibilityElement = true
        backgroundColor = .clear
        textContainerInset = .zero
        textContainer.lineFragmentPadding = 0
        textContainer.widthTracksTextView = true
        textContainer.lineBreakMode = .byWordWrapping
        panGestureRecognizer.isEnabled = false
        if gestureRecognizers?.contains(where: { $0 is PostTapRecognizer }) != true {
            addGestureRecognizer(PostTapRecognizer(target: self, action: #selector(tapped)))
        }
    }

    override var canBecomeFirstResponder: Bool { false }

    func apply(text: String, font: UIFont, lineSpacing: CGFloat, ink: UIColor, link: UIColor) {
        precondition(
            ownedTextStorage === textStorage || ownedTextStorage.layoutManagers.isEmpty,
            "post text lost its text storage"
        )
        let key = "\(text)\u{0}\(font.fontName)\u{0}\(font.pointSize)\u{0}\(lineSpacing)\u{0}\(ink)\u{0}\(link)"
        if key != renderedKey {
            renderedKey = key
            attributedText = postAttributedString(text: text, font: font, lineSpacing: lineSpacing, ink: ink, link: link)
            tintColor = link
            linkTextAttributes = [
                .foregroundColor: link,
                .underlineStyle: NSUnderlineStyle.single.rawValue,
                .underlineColor: link,
            ]
        }
        accessibilityLabel = text
        if onOpen != nil {
            accessibilityTraits.formUnion(.button)
        } else {
            accessibilityTraits.subtract(.button)
        }
        accessibilityCustomActions = linkSpans(text).map { span in
            UIAccessibilityCustomAction(name: span.url.absoluteString) { [weak self] _ in
                self?.onLink?(span.url)
                return true
            }
        }
    }

    /// View coordinates. A link opens the URL and returns without `onOpen`.
    func handleTap(at point: CGPoint) {
        if let url = link(at: point) {
            onLink?(url)
            return
        }
        onOpen?()
    }

    func link(at point: CGPoint) -> URL? {
        guard let attributed = attributedText, attributed.length > 0 else { return nil }
        let location = CGPoint(
            x: point.x - textContainerInset.left,
            y: point.y - textContainerInset.top
        )
        var fraction: CGFloat = 0
        let index = layoutManager.characterIndex(
            for: location,
            in: textContainer,
            fractionOfDistanceBetweenInsertionPoints: &fraction
        )
        guard index < attributed.length else { return nil }
        let glyph = layoutManager.glyphIndexForCharacter(at: index)
        let glyphRect = layoutManager.boundingRect(
            forGlyphRange: NSRange(location: glyph, length: 1),
            in: textContainer
        )
        // `characterIndex` snaps to the nearest glyph, including the margin past a link.
        guard glyphRect.insetBy(dx: -1, dy: -2).contains(location) else { return nil }
        let value = attributed.attribute(.link, at: index, effectiveRange: nil)
        if let url = value as? URL { return url }
        if let string = value as? String { return URL(string: string) }
        return nil
    }

    @objc private func tapped(_ gesture: UITapGestureRecognizer) {
        handleTap(at: gesture.location(in: self))
    }

    override func accessibilityActivate() -> Bool {
        guard let onOpen else { return false }
        onOpen()
        return true
    }
}

func postAttributedString(
    text: String,
    font: UIFont,
    lineSpacing: CGFloat,
    ink: UIColor,
    link: UIColor
) -> NSAttributedString {
    let paragraph = NSMutableParagraphStyle()
    paragraph.lineSpacing = lineSpacing
    paragraph.lineBreakMode = .byWordWrapping
    let result = NSMutableAttributedString(string: text, attributes: [
        .font: font,
        .foregroundColor: ink,
        .paragraphStyle: paragraph,
    ])
    for span in linkSpans(text) {
        let range = NSRange(span.range, in: text)
        guard range.location != NSNotFound, range.length > 0 else { continue }
        result.addAttributes([
            .link: span.url,
            .foregroundColor: link,
            .underlineStyle: NSUnderlineStyle.single.rawValue,
            .underlineColor: link,
        ], range: range)
    }
    return result
}

/// The post body's tap. Tests can inject a point and run the same action the recognizer calls.
final class PostTapRecognizer: UITapGestureRecognizer {
    var injectedLocation: CGPoint?
    override func location(in view: UIView?) -> CGPoint {
        injectedLocation ?? super.location(in: view)
    }
}

private struct PostTextRepresentable: UIViewRepresentable {
    var text: String
    var font: UIFont
    var lineSpacing: CGFloat
    var ink: UIColor
    var link: UIColor
    var onOpen: (() -> Void)?
    var onLink: (URL) -> Void

    func makeUIView(context: Context) -> PostTextView {
        let view = PostTextView(frame: .zero, textContainer: nil)
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        view.setContentHuggingPriority(.required, for: .vertical)
        return view
    }

    func updateUIView(_ uiView: PostTextView, context: Context) {
        uiView.onOpen = onOpen
        uiView.onLink = onLink
        uiView.apply(text: text, font: font, lineSpacing: lineSpacing, ink: ink, link: link)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: PostTextView, context: Context) -> CGSize? {
        let proposed = proposal.width
        let bounded = proposed.flatMap { $0.isFinite && $0 > 0 ? $0 : nil }
        let measureWidth = bounded ?? 10_000
        uiView.textContainer.size = CGSize(width: measureWidth, height: CGFloat.greatestFiniteMagnitude)
        let fitted = uiView.sizeThatFits(CGSize(width: measureWidth, height: CGFloat.greatestFiniteMagnitude))
        let minimum = ceil(uiView.font?.lineHeight ?? font.lineHeight)
        let height = max(ceil(fitted.height), minimum)
        let width = bounded ?? ceil(uiView.layoutManager.usedRect(for: uiView.textContainer).width)
        return CGSize(width: max(width, 1), height: height)
    }
}
