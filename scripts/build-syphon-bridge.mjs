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

// Opt out of the helper entirely (e.g. packaging without Syphon on a machine
// that lacks the Metal Toolchain). forge.config.ts honors the same flag.
if (process.env.SYPHON_SKIP === '1') {
    console.warn('[syphon-bridge] SYPHON_SKIP=1 — skipping helper build.');
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

const UNIVERSAL =
    process.env.UNIVERSAL === '1' || process.argv.includes('--universal');
const FORCE = process.argv.includes('--force');
// Packaging/release scripts pass --require so a missing Metal Toolchain aborts the
// build loudly instead of silently shipping an app without the Syphon helper.
const REQUIRE = process.argv.includes('--require');
const archFlags = UNIVERSAL ? ['--arch', 'arm64', '--arch', 'x86_64'] : [];
// Archs the staged framework + helper must cover: both slices when universal,
// else the host arch (node reports x86_64 as 'x64').
const requestedArchs = UNIVERSAL
    ? ['arm64', 'x86_64']
    : [process.arch === 'arm64' ? 'arm64' : 'x86_64'];

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

/** Whether the staged framework binary already includes every requested arch. */
function frameworkCoversArchs(frameworkPath, archs) {
    try {
        const have = execFileSync('lipo', [
            '-archs',
            join(frameworkPath, 'Syphon'),
        ])
            .toString()
            .trim()
            .split(/\s+/);
        return archs.every((a) => have.includes(a));
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

// 1) Build + stage Syphon.framework (cached unless --force or a requested arch
// is missing — the cache must track arch so a host-only build isn't reused when a
// universal one is requested).
//
// Building Syphon compiles a Metal shader, which on Xcode 26+ needs the
// separately-installed Metal Toolchain. If it is missing and we have no usable
// staged framework, soft-skip so `yarn start` still works — the app runs fine,
// the Syphon menu item is just unavailable until the helper is built. Under
// --require (packaging/release) that same case is fatal instead.
const needFramework =
    FORCE ||
    !existsSync(STAGED_FW) ||
    !frameworkCoversArchs(STAGED_FW, requestedArchs);
if (needFramework) {
    if (!metalToolchainAvailable()) {
        const hint = [
            '[syphon-bridge] Metal Toolchain not installed (needed to compile Syphon shaders on Xcode 26+).',
            '[syphon-bridge] Install it once with: xcodebuild -downloadComponent MetalToolchain',
        ];
        if (REQUIRE) {
            for (const line of hint) console.error(line);
            console.error(
                '[syphon-bridge] --require set — refusing to package without the Syphon helper. Aborting.',
            );
            process.exit(1);
        }
        for (const line of hint) console.warn(line);
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
