// swift-tools-version: 6.3
import PackageDescription

let package = Package(
  name: "SoyehtClient",
  platforms: [.macOS(.v14), .iOS(.v17)],
  products: [.library(name: "SoyehtClient", targets: ["SoyehtClient"])],
  dependencies: [
    .package(path: "../../contracts/swift"),
    .package(url: "https://github.com/soyeht/SwiftTerm", exact: "1.20.0-soyeht.2"),
    .package(url: "https://github.com/swiftlang/swift-markdown", exact: "0.9.0"),
  ],
  targets: [
    // The per-pixel code of the avatar (the rasteriser, the bitmap, the warp's loop). It is
    // compiled optimised in a debug build too: the build a developer runs would otherwise
    // draw the avatar fifty times slower than a release build does. Unsafe flags are allowed
    // here because this package is consumed by path (the Xcode projects and `apps/dictation`).
    .target(
      name: "SoyehtAvatarRaster",
      swiftSettings: [.unsafeFlags(["-O"], .when(configuration: .debug))]
    ),
    .target(
      name: "SoyehtClient",
      dependencies: [
        "SoyehtAvatarRaster",
        .product(name: "SoyehtProtocol", package: "swift"),
        .product(name: "SwiftTerm", package: "SwiftTerm"),
        .product(name: "Markdown", package: "swift-markdown"),
      ],
      resources: [.process("Resources")]
    ),
    .executableTarget(name: "SoyehtIconGenerator", dependencies: ["SoyehtClient"]),
    .testTarget(name: "SoyehtClientTests", dependencies: ["SoyehtClient", "SoyehtAvatarRaster"]),
  ]
)
