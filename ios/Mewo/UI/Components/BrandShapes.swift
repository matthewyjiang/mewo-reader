import SwiftUI

struct TwitterBird: Shape {
    func path(in rect: CGRect) -> Path {
        let sx = rect.width / 248
        let sy = rect.height / 204
        var path = Path()
        path.move(to: CGPoint(x: 221.95 * sx, y: 51.29 * sy))
        path.addCurve(to: CGPoint(x: 222.1 * sx, y: 57.82 * sy), control1: CGPoint(x: 222.1 * sx, y: 53.46 * sy), control2: CGPoint(x: 222.1 * sx, y: 55.63 * sy))
        path.addCurve(to: CGPoint(x: 78.41 * sx, y: 201.51 * sy), control1: CGPoint(x: 222.1 * sx, y: 124.55 * sy), control2: CGPoint(x: 171.3 * sx, y: 201.51 * sy))
        path.addCurve(to: CGPoint(x: 1 * sx, y: 178.87 * sy), control1: CGPoint(x: 50.97 * sx, y: 201.55 * sy), control2: CGPoint(x: 24.1 * sx, y: 193.69 * sy))
        path.addCurve(to: CGPoint(x: 13.02 * sx, y: 179.6 * sy), control1: CGPoint(x: 4.99 * sx, y: 179.35 * sy), control2: CGPoint(x: 9 * sx, y: 179.59 * sy))
        path.addCurve(to: CGPoint(x: 75.74 * sx, y: 157.94 * sy), control1: CGPoint(x: 35.76 * sx, y: 179.62 * sy), control2: CGPoint(x: 57.85 * sx, y: 171.99 * sy))
        path.addCurve(to: CGPoint(x: 28.56 * sx, y: 122.87 * sy), control1: CGPoint(x: 54.13 * sx, y: 157.53 * sy), control2: CGPoint(x: 35.18 * sx, y: 143.44 * sy))
        path.addCurve(to: CGPoint(x: 51.36 * sx, y: 122 * sy), control1: CGPoint(x: 36.13 * sx, y: 124.33 * sy), control2: CGPoint(x: 43.93 * sx, y: 124.03 * sy))
        path.addCurve(to: CGPoint(x: 10.85 * sx, y: 72.5 * sy), control1: CGPoint(x: 27.8 * sx, y: 117.24 * sy), control2: CGPoint(x: 10.85 * sx, y: 96.54 * sy))
        path.addCurve(to: CGPoint(x: 33.77 * sx, y: 78.82 * sy), control1: CGPoint(x: 17.87 * sx, y: 76.41 * sy), control2: CGPoint(x: 25.73 * sx, y: 78.58 * sy))
        path.addCurve(to: CGPoint(x: 18.14 * sx, y: 10.71 * sy), control1: CGPoint(x: 11.58 * sx, y: 63.31 * sy), control2: CGPoint(x: 4.74 * sx, y: 33.79 * sy))
        path.addCurve(to: CGPoint(x: 122.22 * sx, y: 63.47 * sy), control1: CGPoint(x: 43.78 * sx, y: 42.26 * sy), control2: CGPoint(x: 81.61 * sx, y: 61.44 * sy))
        path.addCurve(to: CGPoint(x: 136.83 * sx, y: 15.22 * sy), control1: CGPoint(x: 118.15 * sx, y: 45.93 * sy), control2: CGPoint(x: 123.71 * sx, y: 27.55 * sy))
        path.addCurve(to: CGPoint(x: 208.28 * sx, y: 17.41 * sy), control1: CGPoint(x: 157.17 * sx, y: -3.9 * sy), control2: CGPoint(x: 189.16 * sx, y: -2.92 * sy))
        path.addCurve(to: CGPoint(x: 240.35 * sx, y: 5.15 * sy), control1: CGPoint(x: 219.59 * sx, y: 15.18 * sy), control2: CGPoint(x: 230.43 * sx, y: 11.03 * sy))
        path.addCurve(to: CGPoint(x: 218.15 * sx, y: 33.08 * sy), control1: CGPoint(x: 236.58 * sx, y: 16.84 * sy), control2: CGPoint(x: 228.69 * sx, y: 26.77 * sy))
        path.addCurve(to: CGPoint(x: 247.15 * sx, y: 25.13 * sy), control1: CGPoint(x: 228.16 * sx, y: 31.9 * sy), control2: CGPoint(x: 237.94 * sx, y: 29.22 * sy))
        path.addCurve(to: CGPoint(x: 221.95 * sx, y: 51.29 * sy), control1: CGPoint(x: 240.37 * sx, y: 35.29 * sy), control2: CGPoint(x: 231.83 * sx, y: 44.14 * sy))
        path.closeSubpath()
        return path
    }
}

struct XMark: Shape {
    func path(in rect: CGRect) -> Path {
        let sx = rect.width / 80
        let sy = rect.height / 72
        var path = Path()
        // Official X glyph, translated so it sits in 80x72.
        path.move(to: CGPoint(x: 62.29 * sx, y: 0))
        path.addLine(to: CGPoint(x: 74.42 * sx, y: 0))
        path.addLine(to: CGPoint(x: 47.92 * sx, y: 30.28 * sy))
        path.addLine(to: CGPoint(x: 79.1 * sx, y: 71.5 * sy))
        path.addLine(to: CGPoint(x: 54.69 * sx, y: 71.5 * sy))
        path.addLine(to: CGPoint(x: 35.56 * sx, y: 46.5 * sy))
        path.addLine(to: CGPoint(x: 13.69 * sx, y: 71.5 * sy))
        path.addLine(to: CGPoint(x: 1.56 * sx, y: 71.5 * sy))
        path.addLine(to: CGPoint(x: 29.91 * sx, y: 39.11 * sy))
        path.addLine(to: CGPoint(x: 0 * sx, y: 0))
        path.addLine(to: CGPoint(x: 25.02 * sx, y: 0))
        path.addLine(to: CGPoint(x: 42.3 * sx, y: 22.85 * sy))
        path.addLine(to: CGPoint(x: 62.29 * sx, y: 0))
        path.closeSubpath()
        path.move(to: CGPoint(x: 58.03 * sx, y: 64.24 * sy))
        path.addLine(to: CGPoint(x: 64.76 * sx, y: 64.24 * sy))
        path.addLine(to: CGPoint(x: 21.37 * sx, y: 6.88 * sy))
        path.addLine(to: CGPoint(x: 14.16 * sx, y: 6.88 * sy))
        path.addLine(to: CGPoint(x: 58.03 * sx, y: 64.24 * sy))
        path.closeSubpath()
        return path
    }
}

struct BrandMark: View {
    @Environment(\.mewo) private var mewo
    @Environment(\.app) private var app

    var body: some View {
        if app.mewoMode.enabled {
            Image("MewoTeddy")
                .resizable()
                .scaledToFit()
                .frame(width: 28, height: 28)
                .accessibilityLabel("Mewo")
        } else {
            let twitter = app.themeStore.theme == .twitterLight || app.themeStore.theme == .twitterDark
            let tint = twitter && !mewo.isDark ? twitterBlue : mewo.ink
            Group {
                if twitter {
                    TwitterBird().fill(tint).frame(width: 28, height: 23)
                } else {
                    XMark().fill(tint).frame(width: 24, height: 22)
                }
            }
            .accessibilityLabel("Mewo")
        }
    }
}
