import SwiftUI

enum MewoFont {
    static let regular = "AtkinsonHyperlegible-Regular"
    static let bold = "AtkinsonHyperlegible-Bold"

    static func regular(_ size: CGFloat) -> Font {
        .custom(regular, size: size, relativeTo: .body)
    }

    static func bold(_ size: CGFloat) -> Font {
        .custom(bold, size: size, relativeTo: .headline)
    }
}

struct ReaderTypeScale: Equatable, Sendable {
    static let minStep = 0
    static let maxStep = 6
    static let defaultStep = 2
    static let stepCount = 7
    static let `default` = ReaderTypeScale(step: defaultStep)

    var step: Int

    init(step: Int) {
        precondition((Self.minStep...Self.maxStep).contains(step))
        self.step = step
    }

    var bodySize: CGFloat { CGFloat([14, 15, 17, 19, 21, 24, 28][step]) }
    var bodyLine: CGFloat { CGFloat([20, 22, 24, 27, 30, 34, 39][step]) }
    var headingSize: CGFloat { CGFloat([14, 16, 17, 19, 21, 24, 28][step]) }
    var headingLine: CGFloat { CGFloat([18, 21, 22, 25, 27, 31, 36][step]) }
    var talkBackLabel: String {
        ["Extra small", "Small", "Default", "Medium", "Large", "Extra large", "Largest"][step]
    }

    static func fromStored(_ step: Int) -> ReaderTypeScale {
        (minStep...maxStep).contains(step) ? ReaderTypeScale(step: step) : .default
    }
}

private struct ReaderScaleKey: EnvironmentKey {
    static let defaultValue = ReaderTypeScale.default
}

extension EnvironmentValues {
    var readerScale: ReaderTypeScale {
        get { self[ReaderScaleKey.self] }
        set { self[ReaderScaleKey.self] = newValue }
    }
}
