import SwiftUI

struct CatalogCover: View {
    @Environment(\.mewo) private var mewo
    var title: String
    var coverId: Int?
    var width: CGFloat = 40
    var height: CGFloat = 60

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 3)
        let url = coverUrl(coverId).flatMap(URL.init(string:))
        AsyncImage(url: url) { phase in
            if case .success(let image) = phase {
                image
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            } else {
                placeholder
            }
        }
        .frame(width: width, height: height)
        .clipShape(shape)
        .overlay { shape.strokeBorder(mewo.line, lineWidth: 0.6) }
        .accessibilityHidden(true)
    }

    private var placeholder: some View {
        mewo.hover
            .overlay {
                Text(String(title.prefix(1)))
                    .font(MewoFont.bold(17))
                    .foregroundStyle(mewo.mute)
            }
    }
}
