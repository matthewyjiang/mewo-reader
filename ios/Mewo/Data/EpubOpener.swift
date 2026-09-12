import Foundation
import ReadiumShared
import ReadiumStreamer
import UIKit

struct OpenedMeta: Sendable {
    var title: String
    var author: String
    var cover: UIImage?
}

/// Opens a local EPUB with Readium. PDF support is left out on purpose.
final class EpubOpener: @unchecked Sendable {
    private let httpClient = DefaultHTTPClient()
    private lazy var assetRetriever = AssetRetriever(httpClient: httpClient)
    private lazy var publicationOpener = PublicationOpener(
        parser: DefaultPublicationParser(
            httpClient: httpClient,
            assetRetriever: assetRetriever,
            pdfFactory: DefaultPDFDocumentFactory()
        )
    )

    func open<T: Sendable>(file: URL, use: @escaping @Sendable (Publication) async throws -> T) async throws -> T {
        guard let fileURL = FileURL(url: file) ?? FileURL(string: file.absoluteString) else {
            throw NSError(domain: "mewo", code: 2, userInfo: [NSLocalizedDescriptionKey: "Could not turn \(file.path) into a URL"])
        }
        let asset = try await retrieve(fileURL)
        let publication = try await openPublication(asset)
        return try await use(publication)
    }

    func metadata(file: URL) async throws -> OpenedMeta {
        try await open(file: file) { publication in
            let rawTitle = publication.metadata.title?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            let resolvedTitle = rawTitle.isEmpty
                ? file.deletingPathExtension().lastPathComponent
                : rawTitle
            let author = publication.metadata.authors
                .map(\.name)
                .first(where: { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty })
                ?? "Unknown"
            let cover: UIImage?
            switch await publication.coverFitting(maxSize: CGSize(width: 400, height: 400)) {
            case .success(let image):
                cover = image
            case .failure:
                cover = nil
            }
            return OpenedMeta(title: resolvedTitle, author: author, cover: cover)
        }
    }

    private func retrieve(_ url: FileURL) async throws -> Asset {
        switch await assetRetriever.retrieve(url: url) {
        case .success(let asset):
            return asset
        case .failure(let error):
            throw NSError(domain: "mewo", code: 3, userInfo: [NSLocalizedDescriptionKey: "Could not read EPUB: \(error)"])
        }
    }

    private func openPublication(_ asset: Asset) async throws -> Publication {
        switch await publicationOpener.open(asset: asset, allowUserInteraction: false) {
        case .success(let publication):
            return publication
        case .failure(let error):
            throw NSError(domain: "mewo", code: 4, userInfo: [NSLocalizedDescriptionKey: "Could not open EPUB: \(error)"])
        }
    }
}
