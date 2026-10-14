// swift-tools-version: 6.0

// SwiftPM manifest for building the app with xtool (Linux). Xcode builds use
// project.yml / Mewo.xcodeproj instead; keep dependencies in sync with both.
// Resources are not SwiftPM resources: xtool.yml copies them to the app bundle
// root so the Bundle.main lookups shared with the Xcode build keep working.

import PackageDescription

let package = Package(
    name: "Mewo",
    platforms: [
        .iOS("26.0"),
    ],
    products: [
        // xtool packages this library product as the .app.
        .library(name: "Mewo", targets: ["Mewo"]),
    ],
    dependencies: [
        .package(url: "https://github.com/readium/swift-toolkit.git", from: "3.3.0"),
    ],
    targets: [
        .target(
            name: "Mewo",
            dependencies: [
                .product(name: "ReadiumShared", package: "swift-toolkit"),
                .product(name: "ReadiumStreamer", package: "swift-toolkit"),
            ],
            path: "Mewo",
            exclude: ["Resources"],
            swiftSettings: [
                // Matches SWIFT_VERSION 5.10 in project.yml.
                .swiftLanguageMode(.v5),
            ]
        ),
    ]
)
