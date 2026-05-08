#!/usr/bin/env bash
# Build a .app bundle wrapping the operator-zed binary so macOS treats it as
# a real installed application. Required for the computer-use MCP to grant
# control access — bespoke binaries without a registered .app bundle are
# rejected as "not_installed".
#
# Usage:
#   ./apps/operator-zed/macos/build-app.sh              # build + bundle
#   ./apps/operator-zed/macos/build-app.sh --install    # also copy to /Applications, register, reindex
#   ./apps/operator-zed/macos/build-app.sh --release    # use release profile
#
# The bundle ID dev.danlewis.modz is intentional — keep it stable so
# computer-use MCP grants are persistent.
#
# Why the DT*/NSPrincipalClass fields in Info.plist matter:
# the MCP "is installed" heuristic accepts only bundles with Xcode-style
# build metadata and an NSApplication principal class. A minimal Info.plist
# is rejected even when LaunchServices and Spotlight know about the app.

set -euo pipefail

cd "$(dirname "$0")/../../.."

PROFILE="debug"
INSTALL=0
for arg in "$@"; do
    case "$arg" in
        --release) PROFILE="release" ;;
        --install) INSTALL=1 ;;
        *) echo "unknown arg: $arg" >&2; exit 1 ;;
    esac
done

CARGO_FLAG=()
[[ "$PROFILE" == "release" ]] && CARGO_FLAG+=(--release)

echo "==> cargo build -p operator-zed (${PROFILE})"
cargo build -p operator-zed "${CARGO_FLAG[@]}"

BIN="target/${PROFILE}/operator-zed"
APP_DIR="target/${PROFILE}/Modz.app"
INFO_PLIST="apps/operator-zed/macos/Info.plist"

echo "==> assembling ${APP_DIR}"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
cp "$BIN" "$APP_DIR/Contents/MacOS/modz"
cp "$INFO_PLIST" "$APP_DIR/Contents/Info.plist"

echo "==> ad-hoc codesign"
codesign --force --deep --sign - "$APP_DIR"

if [[ "$INSTALL" -eq 1 ]]; then
    DEST="/Applications/Modz.app"
    echo "==> installing to ${DEST}"
    rm -rf "$DEST"
    cp -R "$APP_DIR" "$DEST"
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$DEST"
    mdimport "$DEST"
    sleep 2
    echo "==> verify"
    mdfind "kMDItemCFBundleIdentifier == 'dev.danlewis.modz'"
fi

echo "==> done"
echo "    open ${APP_DIR} --args path/to/file.modular"
[[ "$INSTALL" -eq 1 ]] && echo "    open /Applications/Modz.app --args path/to/file.modular"
