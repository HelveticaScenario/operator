#!/usr/bin/env node

/**
 * Builds the headless ScreenCaptureKit -> Syphon companion helper
 * (`native/syphon-bridge`) and stages it under `native/syphon-bridge/dist/`
 * mirroring the packaged layout:
 *
 *   dist/MacOS/operator-syphon          <- the helper executable
 *   dist/Frameworks/Syphon.framework    <- vendored Syphon (reached via @rpath ../Frameworks)
 *
 * macOS only — silently skips elsewhere (the feature is macOS-exclusive).
 *
 * Env / flags:
 *   --force      rebuild Syphon.framework even if already staged
 *   UNIVERSAL=1  build arm64 + x86_64 (default: host arch only, for fast dev)
 *
 * Building Syphon from source compiles a Metal shader, which on Xcode 26+ needs
 * the separately-installed Metal Toolchain. We detect its absence and tell the
 * user how to install it rather than failing cryptically.
 */

import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'darwin') {
    console.log('[syphon-bridge] non-macOS platform — skipping.');
    process.exit(0);
}

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
const BRIDGE = join(ROOT, 'native', 'syphon-bridge');
const VENDOR_PROJ = join(
    BRIDGE,
    'vendor',
    'Syphon-Framework',
    'Syphon.xcodeproj',
);
const SYPHON_DERIVED = join(BRIDGE, '.build', 'syphon');
const SYPHON_PRODUCT = join(
    SYPHON_DERIVED,
    'Build',
    'Products',
    'Release',
    'Syphon.framework',
);
const STAGED_FW = join(BRIDGE, 'Frameworks', 'Syphon.framework');
const DIST = join(BRIDGE, 'dist');

const UNIVERSAL = process.env.UNIVERSAL === '1';
const FORCE = process.argv.includes('--force');
const archFlags = UNIVERSAL ? ['--arch', 'arm64', '--arch', 'x86_64'] : [];

function run(cmd, args) {
    console.log(`[syphon-bridge] ${cmd} ${args.join(' ')}`);
    execFileSync(cmd, args, { stdio: 'inherit', cwd: BRIDGE });
}

function metalToolchainAvailable() {
    try {
        execFileSync('xcrun', ['metal', '--version'], { stdio: 'ignore' });
        return true;
    } catch {
        return false;
    }
}

if (!existsSync(VENDOR_PROJ)) {
    console.error(
        '[syphon-bridge] Syphon submodule missing. Run: git submodule update --init --recursive',
    );
    process.exit(1);
}

// 1) Build + stage Syphon.framework (cached unless --force).
//
// Building Syphon compiles a Metal shader, which on Xcode 26+ needs the
// separately-installed Metal Toolchain. If it is missing and we have no
// previously-staged framework, soft-skip so `yarn start` still works — the app
// runs fine, the Syphon menu item is just unavailable until the helper is built.
if (FORCE || !existsSync(STAGED_FW)) {
    if (!metalToolchainAvailable()) {
        console.warn(
            '[syphon-bridge] Metal Toolchain not installed (needed to compile Syphon shaders on Xcode 26+).',
        );
        console.warn(
            '[syphon-bridge] Install it once with: xcodebuild -downloadComponent MetalToolchain',
        );
        console.warn('[syphon-bridge] Skipping Syphon helper build for now.');
        process.exit(0);
    }

    const xcArgs = [
        '-project',
        VENDOR_PROJ,
        '-scheme',
        'Syphon',
        '-configuration',
        'Release',
        '-derivedDataPath',
        SYPHON_DERIVED,
        'DEFINES_MODULE=YES',
        'CODE_SIGNING_ALLOWED=NO',
    ];
    if (UNIVERSAL) xcArgs.push('ARCHS=arm64 x86_64', 'ONLY_ACTIVE_ARCH=NO');
    else xcArgs.push('ONLY_ACTIVE_ARCH=YES');
    xcArgs.push('build');
    run('xcodebuild', xcArgs);

    rmSync(STAGED_FW, { recursive: true, force: true });
    mkdirSync(dirname(STAGED_FW), { recursive: true });
    run('ditto', [SYPHON_PRODUCT, STAGED_FW]);
}

// 2) Build the Swift helper.
run('swift', ['build', '-c', 'release', ...archFlags]);
const binDir = execFileSync(
    'swift',
    ['build', '-c', 'release', ...archFlags, '--show-bin-path'],
    { cwd: BRIDGE },
)
    .toString()
    .trim();
const binPath = join(binDir, 'operator-syphon');

// 3) Assemble dist/ mirroring the packaged bundle layout.
rmSync(DIST, { recursive: true, force: true });
mkdirSync(join(DIST, 'MacOS'), { recursive: true });
mkdirSync(join(DIST, 'Frameworks'), { recursive: true });
cpSync(binPath, join(DIST, 'MacOS', 'operator-syphon'));
run('ditto', [STAGED_FW, join(DIST, 'Frameworks', 'Syphon.framework')]);

console.log(
    `[syphon-bridge] built ${UNIVERSAL ? 'arm64+x86_64' : process.arch} -> ${join(DIST, 'MacOS', 'operator-syphon')}`,
);
