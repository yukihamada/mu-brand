// swift-tools-version: 5.9
import PackageDescription

// Focused, network-free contract tests without building the iOS app or simulator.
// The app itself remains defined by project.yml / MU.xcodeproj.
let package = Package(
    name: "MUContracts",
    platforms: [.macOS(.v13)],
    targets: [
        .target(name: "MU", path: "MU/Sources/Core",
                exclude: ["AIConsent.swift", "AppState.swift", "Push.swift", "Session.swift", "VoiceInput.swift"],
                sources: ["Models.swift", "API.swift"]),
        .testTarget(name: "MUTests", dependencies: ["MU"], path: "MUTests",
                    resources: [.copy("Fixtures")])
    ]
)
