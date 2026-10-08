// swift-tools-version: 6.3
import PackageDescription

let package = Package(
    name: "SoyehtClient",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [.library(name: "SoyehtClient", targets: ["SoyehtClient"])],
    dependencies: [
        .package(path: "../../contracts/swift"),
        .package(url: "https://github.com/migueldeicaza/SwiftTerm", exact: "1.20.0"),
    ],
    targets: [
        .target(
            name: "SoyehtClient",
            dependencies: [
                .product(name: "SoyehtProtocol", package: "swift"),
                .product(name: "SwiftTerm", package: "SwiftTerm"),
            ],
            resources: [.process("Resources")]
        ),
        .executableTarget(name: "SoyehtIconGenerator", dependencies: ["SoyehtClient"]),
        .testTarget(name: "SoyehtClientTests", dependencies: ["SoyehtClient"]),
    ]
)
