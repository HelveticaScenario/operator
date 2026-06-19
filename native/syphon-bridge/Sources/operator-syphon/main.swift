import AppKit
import CoreGraphics
import Foundation
import Metal

// operator-syphon — headless ScreenCaptureKit -> Syphon bridge.
//
// Usage:  operator-syphon <cgWindowID> [serverName] [fps]
//
// Launched by Operator's main process with the target window's CGWindowID
// (parsed from BrowserWindow.getMediaSourceId()). Captures that one window and
// republishes it as a Syphon source. Prints `STATUS=<token>` lines on stdout for
// the parent to react to, and human logs on stderr.

func log(_ message: String) {
    FileHandle.standardError.write(Data("[syphon-bridge] \(message)\n".utf8))
}

/// Machine-readable status for the Electron parent (parsed from stdout).
func emitStatus(_ status: String) {
    FileHandle.standardOutput.write(Data("STATUS=\(status)\n".utf8))
}

struct Args {
    var windowID: CGWindowID
    var serverName: String
    var fps: Int32
}

func parseArgs() -> Args? {
    let argv = CommandLine.arguments
    guard argv.count >= 2, let windowID = UInt32(argv[1]) else { return nil }
    let serverName = argv.count >= 3 ? argv[2] : "Operator"
    let fps = argv.count >= 4 ? (Int32(argv[3]) ?? 60) : 60
    return Args(windowID: windowID, serverName: serverName, fps: max(1, min(fps, 120)))
}

/// Owns the capture/publish wiring and process lifecycle.
///
/// Deliberately a plain (non-`@MainActor`) type: top-level code in main.swift is
/// `@MainActor` by default under Swift 6, so closures authored there would be
/// main-actor-isolated and trap when invoked on the capture or watchdog queues.
/// Defining them here keeps them non-isolated.
///
/// `@unchecked Sendable`: mutable state (signalSources/watchdog/isShuttingDown) is
/// only touched during run()/shutDown() on the main queue, so the lifecycle
/// callbacks can capture `self` without data races.
final class BridgeRunner: @unchecked Sendable {
    private let capture: WindowCapture
    private let publisher: SyphonPublisher
    private let windowID: CGWindowID
    private let fps: Int32
    private var signalSources: [DispatchSourceSignal] = []
    private var watchdog: DispatchSourceTimer?
    private var isShuttingDown = false

    init(
        device: MTLDevice, publisher: SyphonPublisher, windowID: CGWindowID,
        fps: Int32
    ) {
        self.capture = WindowCapture(device: device)
        self.publisher = publisher
        self.windowID = windowID
        self.fps = fps
    }

    func run() {
        capture.onTexture = { [publisher] texture, hold in
            publisher.publish(texture: texture, hold: hold)
        }
        capture.onStop = { [weak self] reason in
            log("capture stopped: \(reason)")
            emitStatus("stopped")
            self?.shutDown()
        }

        installSignalHandlers()
        installParentWatchdog()

        let capture = self.capture
        let windowID = self.windowID
        let fps = self.fps
        Task {
            do {
                try await capture.start(windowID: windowID, fps: fps)
                log("publishing window \(windowID) as a Syphon source @ \(fps)fps")
                emitStatus("ready")
            } catch {
                log("failed to start capture: \(error)")
                emitStatus("error")
                exit(75)  // EX_TEMPFAIL
            }
        }
    }

    /// Idempotent graceful teardown: end the SCStream and retire the Syphon
    /// server (so clients see the source leave), then exit. A hard 1s backstop
    /// guarantees the process exits even if the async stop stalls. All callers
    /// dispatch on the main queue, so the `isShuttingDown` guard needs no lock.
    private func shutDown() {
        if isShuttingDown { return }
        isShuttingDown = true
        watchdog?.cancel()

        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { exit(0) }

        let capture = self.capture
        let publisher = self.publisher
        Task {
            await capture.stop()  // ends the SCStream session
            publisher.stop()  // retires the Syphon server
            exit(0)
        }
    }

    // Operator signals us on quit (SIGTERM) / toggle-off. Handlers run on the
    // main queue, which the run loop drains.
    private func installSignalHandlers() {
        for sig in [SIGTERM, SIGINT] {
            signal(sig, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: sig, queue: .main)
            source.setEventHandler { [weak self] in
                log("received signal \(sig); shutting down")
                self?.shutDown()
            }
            source.resume()
            signalSources.append(source)
        }
    }

    // On macOS an orphaned child is reparented to launchd (pid 1) with no signal,
    // so poll getppid() and tear down if Operator is gone. On the main queue so it
    // serializes with the signal handlers over `isShuttingDown`.
    private func installParentWatchdog() {
        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now() + 2, repeating: 2)
        timer.setEventHandler { [weak self] in
            if getppid() == 1 {
                log("parent process exited; shutting down")
                self?.shutDown()
            }
        }
        timer.resume()
        watchdog = timer
    }
}

// MARK: - entry

guard let args = parseArgs() else {
    log("usage: operator-syphon <cgWindowID> [serverName] [fps]")
    exit(64)  // EX_USAGE
}

guard let device = MTLCreateSystemDefaultDevice() else {
    log("no Metal device available")
    exit(70)  // EX_SOFTWARE
}

// ScreenCaptureKit always requires the Screen Recording grant — even for our own
// app's window. Spawned as a plain child of Operator, the grant is attributed to
// the responsible process (Operator.app), so the user grants "Operator" once.
// The grant only takes effect on a fresh launch, so request it and exit; Operator
// relaunches us after the user grants.
if !CGPreflightScreenCaptureAccess() {
    log("Screen Recording permission not granted; requesting…")
    emitStatus("permission_required")
    _ = CGRequestScreenCaptureAccess()
    exit(2)
}

guard let publisher = SyphonPublisher(device: device, name: args.serverName) else {
    log("could not create Syphon server")
    exit(70)
}

let runner = BridgeRunner(
    device: device, publisher: publisher, windowID: args.windowID, fps: args.fps)
runner.run()

// Syphon announces its server over a Mach port serviced by the run loop; the main
// dispatch queue (signal handlers) is drained by this loop too.
CFRunLoopRun()
