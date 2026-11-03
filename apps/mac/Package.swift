// swift-tools-version: 6.0
// Phase 0 trial programs for the native Mac app (frames_swift_spec.md
// section 14). Run scripts/build-core-xcframework.sh first: it writes the
// Rust core and its Swift bindings to Generated/.
import PackageDescription

let package = Package(
    name: "FramesMacSpikes",
    platforms: [.macOS("26.0")],
    targets: [
        .binaryTarget(name: "FramesCoreFFI", path: "Generated/FramesCoreFFI.xcframework"),
        .target(
            name: "FramesCore",
            dependencies: ["FramesCoreFFI"],
            path: "Generated/FramesCore"
        ),
        .executableTarget(name: "ffi-smoke", dependencies: ["FramesCore"], path: "Spikes/FFISmoke"),
        .executableTarget(
            name: "grid-spike",
            path: "Spikes/GridSpike",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .executableTarget(
            name: "video-spike",
            path: "Spikes/VideoSpike",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
