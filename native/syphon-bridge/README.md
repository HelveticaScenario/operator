# operator-syphon

A headless macOS companion that captures the Operator window with
**ScreenCaptureKit** and republishes it as a **Syphon** source, so the live
patch view can be pulled into Resolume, VDMX, MadMapper, TouchDesigner, Isadora,
etc. Operator launches and stops it from the **View → Publish Window to Syphon**
menu item.

## How it works

1. Operator's main process reads the window's `CGWindowID` from
   `BrowserWindow.getMediaSourceId()` and spawns this helper with
   `operator-syphon <cgWindowID> <serverName> <fps>`.
2. The helper finds that window via `SCShareableContent`, captures it with an
   `SCStream` (`SCContentFilter(desktopIndependentWindow:)`), and converts each
   IOSurface-backed frame to an `MTLTexture` zero-copy via `CVMetalTextureCache`.
3. It publishes the texture through `SyphonMetalServer` under the name
   `Operator`.

The helper is spawned as a plain child of Operator, so macOS attributes the
**Screen Recording** permission to the responsible process (Operator) — the user
grants "Operator" once. The helper exits with code 2 the first time if the grant
is missing (after triggering the prompt); Operator surfaces a dialog and the
user re-enables the menu item once granted.

It self-terminates if Operator dies (it polls `getppid()`), and Operator also
signals it on quit.

## Layout

```
Package.swift                      SwiftPM executable; links ScreenCaptureKit/Metal
                                   + vendored Syphon via -F/-framework + @rpath
Info.plist                         CFBundleIdentifier + LSUIElement (no Dock icon)
Sources/operator-syphon/
  main.swift                       arg parsing, permission flow, lifecycle, run loop
  WindowCapture.swift              ScreenCaptureKit -> zero-copy MTLTexture
  SyphonPublisher.swift            SyphonMetalServer wrapper
vendor/Syphon-Framework/           git submodule (built from source)
```

`Frameworks/`, `dist/`, and `.build/` are generated and git-ignored.

## Building

```bash
yarn build-syphon-bridge          # from the repo root
```

This builds Syphon.framework (cached after the first run), compiles the helper,
and stages `dist/MacOS/operator-syphon` + `dist/Frameworks/Syphon.framework`
mirroring the packaged bundle layout. It also runs automatically as part of
`yarn start`, `yarn package`, and `yarn make`.

Pass `UNIVERSAL=1` to build a universal (arm64 + x86_64) helper for distribution;
the default is the host arch for fast dev iteration.

### Metal Toolchain requirement

Building Syphon from source compiles a Metal shader. On **Xcode 26+** the Metal
compiler is a separately-installed component. If it's missing, install it once:

```bash
xcodebuild -downloadComponent MetalToolchain
```

`build-syphon-bridge` soft-skips (warns and continues) when the toolchain is
absent and no framework has been staged yet, so `yarn start` still works — the
Syphon menu item is simply unavailable until the helper is built. CI that
produces release builds must have the Metal Toolchain installed.

## Packaging & signing

`forge.config.ts`'s `packageAfterCopy` hook stages the helper into
`Contents/MacOS/operator-syphon` and Syphon.framework into
`Contents/Frameworks/`. `@electron/osx-sign` then signs both inside-out with the
app's Developer ID under the hardened runtime (re-signing Syphon with the same
team so library validation passes), and the whole bundle is notarized.
