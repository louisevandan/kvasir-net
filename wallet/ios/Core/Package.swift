// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Core",
    platforms: [
        .iOS(.v16),
        .macOS(.v13),
    ],
    products: [
        .library(name: "Core", targets: ["Core"]),
    ],
    dependencies: [
        .package(url: "https://github.com/p2p-org/solana-swift", from: "5.0.0"),
        // ed25519 detached signing for the hub Sign-In-With-Solana node-token flow
        // (same package solana-swift already resolves, so no new version to pin).
        .package(url: "https://github.com/bitmark-inc/tweetnacl-swiftwrap.git", from: "1.0.2"),
    ],
    targets: [
        .target(
            name: "Core",
            dependencies: [
                .product(name: "SolanaSwift", package: "solana-swift"),
                .product(name: "TweetNacl", package: "tweetnacl-swiftwrap"),
            ],
            resources: [
                .process("Resources"),
            ]
        ),
        .testTarget(
            name: "CoreTests",
            dependencies: ["Core"]
        ),
    ]
)
