import AppKit
import CoreMedia
import CoreVideo
import Foundation
import Metal
import ScreenCaptureKit

enum CaptureError: Error, CustomStringConvertible {
    case windowNotFound(CGWindowID)
    case noMetalTexture

    var description: String {
        switch self {
        case .windowNotFound(let id): return "window \(id) not found / not shareable"
        case .noMetalTexture: return "could not create Metal texture from frame"
        }
    }
}

/// Captures exactly one on-screen window (by `CGWindowID`) with ScreenCaptureKit
/// and converts each frame to a zero-copy `MTLTexture` (the texture aliases the
/// capture's IOSurface via `CVMetalTextureCache`).
///
/// `@unchecked Sendable`: configured once on the main thread before `start`, then
/// frames are delivered on a single serial queue, so there is no concurrent
/// mutation to guard.
final class WindowCapture: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
    private let device: MTLDevice
    private var textureCache: CVMetalTextureCache?
    private var stream: SCStream?
    private let sampleQueue = DispatchQueue(label: "dev.operator.syphon.capture", qos: .userInteractive)

    /// Called on the sample queue for each complete frame.
    var onTexture: ((MTLTexture, CVMetalTexture) -> Void)?
    /// Called if the stream stops on its own (e.g. the target window closed).
    var onStop: ((String) -> Void)?

    init(device: MTLDevice) {
        self.device = device
        super.init()
        let status = CVMetalTextureCacheCreate(
            kCFAllocatorDefault, nil, device, nil, &textureCache)
        // Without the cache every frame's texture lookup fails and the source
        // would publish nothing while still reporting "ready" — fail loudly
        // instead, matching the other fatal setup guards in main.swift.
        if status != kCVReturnSuccess || textureCache == nil {
            log("CVMetalTextureCacheCreate failed: \(status)")
            emitStatus("error")
            exit(70)  // EX_SOFTWARE
        }
    }

    func start(windowID: CGWindowID, fps: Int32) async throws {
        let scWindow = try await findWindow(windowID: windowID)
        let filter = SCContentFilter(desktopIndependentWindow: scWindow)

        // Capture at the window's backing-pixel resolution, using the scale of
        // the display the window actually sits on (not necessarily the key
        // window's screen), so a window on a non-Retina external display isn't
        // captured at the wrong DPI.
        let scale = Self.backingScale(for: scWindow.frame)
        let config = SCStreamConfiguration()
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.width = max(2, Int(scWindow.frame.width * scale))
        config.height = max(2, Int(scWindow.frame.height * scale))
        config.colorSpaceName = CGColorSpace.sRGB
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
        config.queueDepth = 5
        config.showsCursor = false
        config.scalesToFit = true

        let stream = SCStream(filter: filter, configuration: config, delegate: self)
        try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: sampleQueue)
        try await stream.startCapture()
        self.stream = stream
    }

    func stop() async {
        try? await stream?.stopCapture()
        // stopCapture() halts delivery, but a sample handler may still be running
        // on sampleQueue. Drain it (the queue is serial) so no publish() is in
        // flight before the caller retires the Syphon server, otherwise that copy
        // would race server.stop(). stop() runs off the sample queue, so this
        // cannot deadlock.
        sampleQueue.sync {}
        stream = nil
    }

    /// Backing scale of the display the window's centre lies on.
    ///
    /// `SCWindow.frame` is in top-left-origin global points; `NSScreen.frame` is
    /// bottom-left-origin. Both spaces are anchored on the primary screen (the
    /// one at AppKit origin `(0, 0)`, i.e. `screens.first`), so flipping the
    /// centre's y by the primary screen's height converts between them. Falls
    /// back to the main screen's scale (then 2) if no screen contains the centre.
    private static func backingScale(for windowFrame: CGRect) -> CGFloat {
        let screens = NSScreen.screens
        guard let primary = screens.first else { return 2 }
        let centerAppKit = CGPoint(
            x: windowFrame.midX,
            y: primary.frame.height - windowFrame.midY)
        for screen in screens where screen.frame.contains(centerAppKit) {
            return screen.backingScaleFactor
        }
        return NSScreen.main?.backingScaleFactor ?? primary.backingScaleFactor
    }

    /// The window may not be shareable the instant we launch (it can lag the
    /// Electron window appearing), so retry the lookup briefly.
    private func findWindow(windowID: CGWindowID, attempts: Int = 10) async throws -> SCWindow {
        var lastError: Error?
        for _ in 0..<attempts {
            do {
                let content = try await SCShareableContent.excludingDesktopWindows(
                    false, onScreenWindowsOnly: true)
                if let window = content.windows.first(where: { $0.windowID == windowID }) {
                    return window
                }
            } catch {
                lastError = error
            }
            try? await Task.sleep(nanoseconds: 300_000_000)
        }
        throw lastError ?? CaptureError.windowNotFound(windowID)
    }

    // MARK: SCStreamOutput

    func stream(
        _ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
        of type: SCStreamOutputType
    ) {
        guard type == .screen, sampleBuffer.isValid else { return }

        // Only complete frames carry fresh pixels; idle/blank/suspended frames
        // reuse a stale buffer and would publish a frozen or black image.
        guard
            let attachments = CMSampleBufferGetSampleAttachmentsArray(
                sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
            let info = attachments.first,
            let statusRaw = info[.status] as? Int,
            SCFrameStatus(rawValue: statusRaw) == .complete
        else { return }

        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer),
            CVPixelBufferGetIOSurface(pixelBuffer) != nil,
            let textureCache
        else { return }

        let width = CVPixelBufferGetWidth(pixelBuffer)
        let height = CVPixelBufferGetHeight(pixelBuffer)

        var cvTexture: CVMetalTexture?
        let result = CVMetalTextureCacheCreateTextureFromImage(
            kCFAllocatorDefault, textureCache, pixelBuffer, nil,
            .bgra8Unorm, width, height, 0, &cvTexture)
        guard result == kCVReturnSuccess, let cvTexture,
            let mtlTexture = CVMetalTextureGetTexture(cvTexture)
        else { return }

        onTexture?(mtlTexture, cvTexture)
    }

    // MARK: SCStreamDelegate

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        onStop?(error.localizedDescription)
    }
}
