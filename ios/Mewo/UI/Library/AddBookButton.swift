import SwiftUI

enum AddBookOption: String, Equatable, Identifiable, Sendable {
    case upload
    case catalog

    var id: String { rawValue }

    var title: String {
        switch self {
        case .upload: return "Upload EPUB"
        case .catalog: return "Browse catalog"
        }
    }
}

func addBookOptions(canRequest: Bool) -> [AddBookOption] {
    canRequest ? [.upload, .catalog] : [.upload]
}

struct AddBookButton<Label: View>: View {
    var canRequest: Bool
    var identifier: String
    var accessibilityLabel: String?
    var onUpload: () -> Void
    var onBrowseCatalog: () -> Void
    @ViewBuilder var label: () -> Label

    var body: some View {
        let options = addBookOptions(canRequest: canRequest)
        Group {
            if options.count > 1 {
                Menu {
                    Button(AddBookOption.upload.title, action: onUpload)
                    Button(AddBookOption.catalog.title, action: onBrowseCatalog)
                } label: {
                    label()
                }
                .buttonStyle(.plain)
            } else {
                Button(action: onUpload, label: label)
                    .buttonStyle(.plain)
            }
        }
        .accessibilityIdentifier(identifier)
        .modifier(AccessLabel(text: accessibilityLabel))
    }
}

private struct AccessLabel: ViewModifier {
    var text: String?

    func body(content: Content) -> some View {
        if let text {
            content.accessibilityLabel(text)
        } else {
            content
        }
    }
}
