import Foundation

enum CatalogAccess: Equatable, Sendable {
    case unknown, off, on
}

enum CatalogErrorReason: Equatable, Sendable {
    case unauthorized
    case forbidden
    case notFound
    case network
    case unreadable
    case off
    case notSearch
    case http(Int)
}

/// `message` is already something a person can read. Callers show it as-is.
struct CatalogError: LocalizedError, Equatable, Sendable {
    var reason: CatalogErrorReason
    var message: String
    var errorDescription: String? { message }
}

struct Me: Decodable, Equatable, Sendable {
    var username: String
    var isAdmin: Bool
    var canRequest: Bool
}

struct BookRequest: Decodable, Equatable, Sendable {
    var id: String
    var workKey: String
    var title: String
    var author: String
    var year: Int?
    var coverId: Int?
    var state: String
    var progress: Int
    var error: String?
    var bookId: String?
    var requester: String
    var createdAt: Int64
    var updatedAt: Int64
    var follows: String?
}

struct ServerCopy: Decodable, Equatable, Sendable {
    var bookId: String
    var ownerUsername: String
    var mine: Bool
    var shared: Bool
    var held: Bool
}

struct CatalogWork: Decodable, Equatable, Identifiable, Sendable {
    var workKey: String
    var title: String
    var author: String
    var year: Int?
    var coverId: Int?
    var request: BookRequest?
    var onServer: ServerCopy?
    var id: String { workKey }
}

struct NewBookRequest: Encodable, Equatable, Sendable {
    var workKey: String
    var title: String
    var author: String
    var year: Int?
    var coverId: Int?

    init(workKey: String, title: String, author: String, year: Int?, coverId: Int?) {
        self.workKey = workKey
        self.title = title
        self.author = author
        self.year = year
        self.coverId = coverId
    }

    init(_ work: CatalogWork) {
        self.init(workKey: work.workKey, title: work.title, author: work.author, year: work.year, coverId: work.coverId)
    }
}

struct CatalogPage: Decodable, Equatable, Sendable {
    var items: [CatalogWork]
    var nextPage: Int?
}

struct Rating: Decodable, Equatable, Sendable {
    var average: Double
    var count: Int
}

struct WorkInfo: Decodable, Equatable, Sendable {
    var canonicalKey: String
    var title: String
    var subtitle: String?
    var authors: [String]
    var year: Int?
    var coverId: Int?
    var description: String?
    var subjects: [String]
    var awards: [String]
    var series: [String]
    var editionCount: Int?
    var pagesMedian: Int?
    var languages: [String]
    var hasAcceptedEdition: Bool
    var rating: Rating?
}

struct WorkStatus: Decodable, Equatable, Sendable {
    var request: RequestDetail?
    var onServer: ServerCopy?
    var active: Int
    var cap: Int
    var otherRequests: [BookRequest]
}

struct WorkDetail: Decodable, Equatable, Sendable {
    var workKey: String
    var info: WorkInfo?
    var infoError: String?
    var status: WorkStatus
}

/// `GET /v1/catalog/works/{olid}` flattens the book request beside these fields.
struct RequestDetail: Equatable, Sendable {
    var request: BookRequest
    var attempts: Int
    var maxAttempts: Int
    var nextCheckAt: Int64?
    var releaseTitle: String?
    var releaseSize: Int64?
    var phase: String
}

extension RequestDetail: Decodable {
    enum CodingKeys: String, CodingKey {
        case attempts, maxAttempts, nextCheckAt, releaseTitle, releaseSize, phase
    }

    init(from decoder: Decoder) throws {
        request = try BookRequest(from: decoder)
        let container = try decoder.container(keyedBy: CodingKeys.self)
        attempts = try container.decode(Int.self, forKey: .attempts)
        maxAttempts = try container.decode(Int.self, forKey: .maxAttempts)
        nextCheckAt = try container.decodeIfPresent(Int64.self, forKey: .nextCheckAt)
        releaseTitle = try container.decodeIfPresent(String.self, forKey: .releaseTitle)
        releaseSize = try container.decodeIfPresent(Int64.self, forKey: .releaseSize)
        phase = try container.decode(String.self, forKey: .phase)
    }
}
