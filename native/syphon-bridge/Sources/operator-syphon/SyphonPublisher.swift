import CoreVideo
import Foundation
import Metal
import Syphon

/// Publishes Metal textures as a Syphon source via `SyphonMetalServer`.
///
/// `SyphonMetalServer.publishFrameTexture(_:onCommandBuffer:imageRegion:flipped:)`
/// schedules a GPU copy of the supplied texture onto Syphon's own IOSurface, so
/// the caller-owned source surface must stay alive until that command buffer
/// completes (see `publish`).
/// `@unchecked Sendable`: `server`/`commandQueue` are immutable after init;
/// `publish` runs on the capture queue and `stop` on the main thread.
final class SyphonPublisher: @unchecked Sendable {
    private let server: SyphonMetalServer
    private let commandQueue: MTLCommandQueue

    init?(device: MTLDevice, name: String) {
        guard let queue = device.makeCommandQueue() else { return nil }
        self.commandQueue = queue
        self.server = SyphonMetalServer(name: name, device: device, options: nil)
    }

    /// Publish one frame. `hold` is the `CVMetalTexture` that the source
    /// `MTLTexture` aliases; retaining it until the command buffer finishes keeps
    /// the backing IOSurface valid while Syphon copies from it.
    func publish(texture: MTLTexture, hold: CVMetalTexture) {
        // No connected clients means nothing reads the surface — skip the GPU work.
        guard server.hasClients else { return }
        guard let commandBuffer = commandQueue.makeCommandBuffer() else { return }

        let region = NSRect(x: 0, y: 0, width: texture.width, height: texture.height)
        // ScreenCaptureKit frames are top-left origin, which Syphon calls "flipped".
        server.publishFrameTexture(
            texture,
            on: commandBuffer,
            imageRegion: region,
            flipped: true
        )
        // Extend the source surface's lifetime past the async GPU copy. CVMetalTexture
        // isn't Sendable; the capture only retains it, so opting out is safe.
        nonisolated(unsafe) let held = hold
        commandBuffer.addCompletedHandler { _ in
            _ = held
        }
        commandBuffer.commit()
    }

    func stop() {
        server.stop()
    }
}
