// swift-tools-version: 6.3
import PackageDescription

let package = Package(
    name: "SoyehtProtocol",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "SoyehtProtocol", targets: ["SoyehtProtocol"]),
        .executable(name: "protocol-vectors", targets: ["ProtocolVectors"]),
    ],
    targets: [
        .target(name: "SoyehtProtocol"),
        .executableTarget(name: "ProtocolVectors", dependencies: ["SoyehtProtocol"]),
    ]
)
