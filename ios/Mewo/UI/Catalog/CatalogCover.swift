import SwiftUI

struct CatalogCover: View {
    @Environment(\.mewo) private var mewo
    var title: String
    var coverId: Int?
    var width: CGFloat = 44
    var height: CGFloat = 66

    var body: some View {
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
        .clipShape(RoundedRectangle(cornerRadius: 4))
        .accessibilityHidden(true)
    }

    private var placeholder: some View {
        RoundedRectangle(cornerRadius: 4)
            .fill(mewo.hover)
            .overlay {
                Text(String(title.prefix(1)))
                    .font(MewoFont.bold(15))
                    .foregroundStyle(mewo.mute)
            }
    }
}
