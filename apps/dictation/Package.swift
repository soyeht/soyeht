// swift-tools-version: 6.3
import PackageDescription

let package = Package(
    name: "SoyehtDictation",
    platforms: [.macOS(.v15), .iOS(.v18)],
    products: [.library(name: "SoyehtDictation", targets: ["SoyehtDictation"])],
    dependencies: [
        .package(path: "../shared"),
        .package(
            url: "https://github.com/soniqo/speech-swift",
            revision: "2723d1c2754fae2921fd4c475eb2594e88f7de39"),
    ],
    targets: [
        .target(
            name: "SoyehtDictation",
            dependencies: [
                .product(name: "SoyehtClient", package: "shared"),
                .product(name: "AudioCommon", package: "speech-swift"),
                .product(name: "NemotronStreamingASR", package: "speech-swift"),
            ]
        ),
        .testTarget(name: "SoyehtDictationTests", dependencies: ["SoyehtDictation"]),
    ]
)
