#!/usr/bin/env node

/**
 * Patches the development Electron.app Info.plist with Local Network
 * permission keys required for Ableton Link (mDNS/Bonjour discovery), the
 * camera usage string that `$v.camera` needs for the system to ask, and
 * names the bundle "Operator Dev (<checkout folder>)" so the macOS menu bar
 * and Dock tell dev builds from different worktrees apart. The name matches
 * the dev app name set in src/main/main.ts.
 *
 * The name is a localized display name: macOS shows it only while the base
 * CFBundleName/CFBundleDisplayName still match the bundle's file name
 * ("Electron"), so those stay untouched.
 *
 * macOS only — silently skips on other platforms.
 * Runs on `yarn install` and `yarn start`; re-signs only when the plist changes.
 */

import { execFileSync } from 'child_process';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'fs';
import { basename, resolve } from 'path';

if (process.platform !== 'darwin') {
    process.exit(0);
}

const root = resolve(import.meta.dirname, '..');
const electronApp = resolve(
    root,
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
const bundleName = `Operator Dev (${basename(root)})`;

function plistBuddy(command) {
    try {
        return execFileSync('/usr/libexec/PlistBuddy', ['-c', command, plist], {
            encoding: 'utf8',
            stdio: 'pipe',
        }).trim();
    } catch {
        return null;
    }
}

let changed = false;

function setString(key, value) {
    const current = plistBuddy(`Print :${key}`);
    if (current === value) return;
    plistBuddy(
        current === null
            ? `Add :${key} string '${value}'`
            : `Set :${key} '${value}'`,
    );
    changed = true;
    console.log(`[patch-electron-plist] Set ${key}`);
}

setString('NSLocalNetworkUsageDescription', description);
setString(
    'NSCameraUsageDescription',
    'Operator shows the camera in video patches that use $v.camera.',
);
setString('CFBundleName', 'Electron');
setString('CFBundleDisplayName', 'Electron');

if (plistBuddy('Print :LSHasLocalizedDisplayName') !== 'true') {
    plistBuddy('Delete :LSHasLocalizedDisplayName');
    plistBuddy('Add :LSHasLocalizedDisplayName bool true');
    changed = true;
    console.log('[patch-electron-plist] Set LSHasLocalizedDisplayName');
}

const resources = resolve(electronApp, 'Contents', 'Resources');
const infoPlistStrings =
    `"CFBundleName" = ${JSON.stringify(bundleName)};\n` +
    `"CFBundleDisplayName" = ${JSON.stringify(bundleName)};\n`;
for (const entry of readdirSync(resources)) {
    if (!entry.endsWith('.lproj')) continue;
    const file = resolve(resources, entry, 'InfoPlist.strings');
    if (existsSync(file) && readFileSync(file, 'utf8') === infoPlistStrings) {
        continue;
    }
    writeFileSync(file, infoPlistStrings);
    changed = true;
}

if (plistBuddy(`Print :NSBonjourServices`) === null) {
    plistBuddy(`Add :NSBonjourServices array`);
    plistBuddy(`Add :NSBonjourServices:0 string '${bonjourService}'`);
    changed = true;
    console.log('[patch-electron-plist] Added NSBonjourServices');
}

// LaunchServices caches the bundle name the Dock shows, so refresh it even when
// the plist is already patched.
function registerWithLaunchServices() {
    try {
        execFileSync(
            '/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister',
            ['-f', electronApp],
            { stdio: 'pipe' },
        );
    } catch (err) {
        console.warn(
            '[patch-electron-plist] Warning: failed to register Electron.app with LaunchServices:',
            err.message,
        );
    }
}

if (!changed) {
    registerWithLaunchServices();
    process.exit(0);
}

// Re-sign the app bundle with an ad-hoc signature.
// Modifying Info.plist invalidates the existing code signature, and macOS
// silently blocks Local Network access (no prompt) for unsigned/broken-sig apps.
try {
    execFileSync(
        'codesign',
        ['--force', '--deep', '--sign', '-', electronApp],
        {
            stdio: 'pipe',
        },
    );
    console.log('[patch-electron-plist] Re-signed Electron.app (ad-hoc)');
} catch (err) {
    console.warn(
        '[patch-electron-plist] Warning: failed to re-sign Electron.app:',
        err.message,
    );
}

registerWithLaunchServices();
