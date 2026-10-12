// swift-tools-version: 6.3
import PackageDescription

let package = Package(
    name: "SoyehtClient",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [.library(name: "SoyehtClient", targets: ["SoyehtClient"])],
    dependencies: [
        .package(path: "../../contracts/swift"),
        .package(url: "https://github.com/migueldeicaza/SwiftTerm", exact: "1.20.0"),
        .package(url: "https://github.com/swiftlang/swift-markdown", exact: "0.9.0"),
    ],
    targets: [
        .target(
            name: "SoyehtClient",
            dependencies: [
                .product(name: "SoyehtProtocol", package: "swift"),
                .product(name: "SwiftTerm", package: "SwiftTerm"),
                .product(name: "Markdown", package: "swift-markdown"),
            ],
            resources: [.process("Resources")]
        ),
        .executableTarget(name: "SoyehtIconGenerator", dependencies: ["SoyehtClient"]),
        .testTarget(name: "SoyehtClientTests", dependencies: ["SoyehtClient"]),
    ]
)
