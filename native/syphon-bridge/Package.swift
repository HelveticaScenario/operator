// swift-tools-version:6.0
import PackageDescription
import Foundation

// Absolute paths anchored to this manifest so the linker/compiler find the
// vendored Syphon.framework regardless of the working directory SwiftPM uses.
let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
let frameworks = "\(root)/Frameworks"
let infoPlist = "\(root)/Info.plist"

let package = Package(
    name: "operator-syphon",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "operator-syphon",
            path: "Sources/operator-syphon",
            swiftSettings: [
                // Framework search path for `import Syphon`.
                .unsafeFlags(["-F", frameworks])
            ],
            linkerSettings: [
                .linkedFramework("ScreenCaptureKit"),
                .linkedFramework("Metal"),
                .linkedFramework("CoreVideo"),
                .linkedFramework("CoreMedia"),
                .linkedFramework("AppKit"),
                .unsafeFlags([
                    "-F", frameworks,
                    "-framework", "Syphon",
                    // Packaged layout: helper in Contents/MacOS, framework in
                    // Contents/Frameworks. The dev `dist/` mirror uses the same
                    // relative layout so this single rpath resolves in both.
                    "-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks",
                    // Embed an Info.plist (CFBundleIdentifier for a stable code-sign
                    // identity + LSUIElement so it never shows a Dock icon).
                    "-Xlinker", "-sectcreate",
                    "-Xlinker", "__TEXT",
                    "-Xlinker", "__info_plist",
                    "-Xlinker", infoPlist,
                ]),
            ]
        )
    ]
)
