#!/usr/bin/env node

/**
 * Patches the development Electron.app Info.plist with Local Network
 * permission keys required for Ableton Link (mDNS/Bonjour discovery).
 *
 * macOS only — silently skips on other platforms.
 * Re-run after `yarn install` or Electron version changes.
 */

import { execSync } from 'child_process';
import { existsSync } from 'fs';
import { resolve } from 'path';

if (process.platform !== 'darwin') {
    process.exit(0);
}

const electronApp = resolve(
    import.meta.dirname,
    '..',
    'node_modules',
    'electron',
    'dist',
    'Electron.app',
);

const plist = resolve(electronApp, 'Contents', 'Info.plist');

if (!existsSync(plist)) {
    console.warn(
        '[patch-electron-plist] Electron.app not found — skipping plist patch.',
    );
    process.exit(0);
}

const description =
    'Operator uses the local network to sync tempo with other music apps via Ableton Link.';
const bonjourService = '_SessionStatus._tcp';

function plistBuddy(command) {
    try {
        execSync(`/usr/libexec/PlistBuddy -c "${command}" "${plist}"`, {
            stdio: 'pipe',
        });
        return true;
    } catch {
        return false;
    }
}

// Add NSLocalNetworkUsageDescription (skip if already present)
if (!plistBuddy(`Print :NSLocalNetworkUsageDescription`)) {
    plistBuddy(
        `Add :NSLocalNetworkUsageDescription string '${description}'`,
    );
    console.log('[patch-electron-plist] Added NSLocalNetworkUsageDescription');
} else {
    plistBuddy(
        `Set :NSLocalNetworkUsageDescription '${description}'`,
    );
    console.log(
        '[patch-electron-plist] Updated NSLocalNetworkUsageDescription',
    );
}

// Add NSBonjourServices array (skip if already present)
if (!plistBuddy(`Print :NSBonjourServices`)) {
    plistBuddy(`Add :NSBonjourServices array`);
    plistBuddy(`Add :NSBonjourServices:0 string '${bonjourService}'`);
    console.log('[patch-electron-plist] Added NSBonjourServices');
} else {
    console.log(
        '[patch-electron-plist] NSBonjourServices already present — skipping',
    );
}

// Re-sign the app bundle with an ad-hoc signature.
// Modifying Info.plist invalidates the existing code signature, and macOS
// silently blocks Local Network access (no prompt) for unsigned/broken-sig apps.
try {
    execSync(
        `codesign --force --deep --sign - "${electronApp}"`,
        { stdio: 'pipe' },
    );
    console.log('[patch-electron-plist] Re-signed Electron.app (ad-hoc)');
} catch (err) {
    console.warn(
        '[patch-electron-plist] Warning: failed to re-sign Electron.app:',
        err.message,
    );
}
