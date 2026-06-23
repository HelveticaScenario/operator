import type { ForgeConfig } from '@electron-forge/shared-types';
import { MakerSquirrel } from '@electron-forge/maker-squirrel';
import { MakerZIP } from '@electron-forge/maker-zip';
import { MakerDeb } from '@electron-forge/maker-deb';
import { MakerRpm } from '@electron-forge/maker-rpm';
// import { MakerFlatpak } from '@electron-forge/maker-flatpak';
import { PublisherGithub } from '@electron-forge/publisher-github';
import { AutoUnpackNativesPlugin } from '@electron-forge/plugin-auto-unpack-natives';
import { VitePlugin } from '@electron-forge/plugin-vite';
import { FusesPlugin } from '@electron-forge/plugin-fuses';
import { FuseV1Options, FuseVersion } from '@electron/fuses';
import * as fs from 'fs';
import * as path from 'path';
import { execFileSync } from 'child_process';

// if (process.env.APPLE_ID && process.env.APPLE_PASSWORD && process.env.APPLE_TEAM_ID) {

const config: ForgeConfig = {
    packagerConfig: {
        asar: {
            // Unpack .node binaries so dlopen/Node addon loader can find them
            // on the filesystem, plus the matching debug-info bundles so
            // std::backtrace can resolve file:line for production panic logs:
            //   - macOS:   `<name>.node.dSYM/...` (lookup by co-location)
            //   - Windows: `<name>.node.pdb`     (lookup by co-location)
            //   - Linux:   embedded in the `.node`, nothing extra to unpack.
            unpack: '**/*.{node,node.pdb,dSYM,dSYM/**}',
        },
        executableName: 'Operator',
        extendInfo: {
            NSLocalNetworkUsageDescription:
                'Operator uses the local network to sync tempo with other music apps via Ableton Link.',
            NSBonjourServices: ['_SessionStatus._tcp'],
        },
        osxSign: {
            identity: 'Developer ID Application: Daniel Lewis (HA98TTLCR7)',
            // Fail the build if any file fails to codesign. @electron/packager
            // defaults this to true, which silently downgrades a signing failure
            // to a warning and ships an unsigned app on to notarization, where it
            // surfaces as a confusing "code has no resources" rejection instead of
            // the real codesign error. Fail loudly at the sign step instead.
            continueOnError: false,
            optionsForFile: (filePath: string) => {
                // The headless Syphon helper and its framework need none of the
                // Electron app's entitlements (JIT, unsigned-executable memory,
                // network, audio) — screen capture is a TCC grant, not an
                // entitlement — so sign them least-privilege under the hardened
                // runtime instead of inheriting the app's broad set.
                const isSyphon =
                    filePath.endsWith('/Resources/operator-syphon') ||
                    filePath.includes('/Frameworks/Syphon.framework');
                const entitlements = isSyphon
                    ? 'native/syphon-bridge/syphon-entitlements.plist'
                    : 'entitlements.plist';
                return {
                    hardenedRuntime: true,
                    entitlements,
                    'entitlements-inherit': entitlements,
                    signatureFlags: 'library',
                };
            },
        },

        // macOS notarization configuration
        // Only runs when environment variables are present (i.e., in CI)
        osxNotarize:
            process.env.APPLE_ID &&
            process.env.APPLE_PASSWORD &&
            process.env.APPLE_TEAM_ID
                ? {
                      appleId: process.env.APPLE_ID,
                      appleIdPassword: process.env.APPLE_PASSWORD,
                      teamId: process.env.APPLE_TEAM_ID,
                  }
                : undefined,
    },
    rebuildConfig: {},
    hooks: {
        // Copy @modular/core workspace package to node_modules before packaging
        // This is needed because yarn workspaces use symlinks which don't survive packaging
        packageAfterCopy: async (
            _config,
            buildPath,
            _electronVersion,
            platform,
            arch,
        ) => {
            const sourceDir = path.join(__dirname, 'crates', 'modular');
            const targetDir = path.join(
                buildPath,
                'node_modules',
                '@modular',
                'core',
            );

            // Ensure target directory exists
            fs.mkdirSync(targetDir, { recursive: true });

            // Files to copy from the native module package
            const filesToCopy = ['index.js', 'index.d.ts', 'package.json'];

            for (const file of filesToCopy) {
                const src = path.join(sourceDir, file);
                const dest = path.join(targetDir, file);
                if (fs.existsSync(src)) {
                    fs.copyFileSync(src, dest);
                }
            }

            // Copy the native .node file for the current platform
            const nodeFiles = fs
                .readdirSync(sourceDir)
                .filter((f) => f.endsWith('.node'));
            for (const nodeFile of nodeFiles) {
                fs.copyFileSync(
                    path.join(sourceDir, nodeFile),
                    path.join(targetDir, nodeFile),
                );
                // Copy the platform-specific debug-info bundle next to the
                // .node so std::backtrace resolves file:line in production
                // panic logs. Bundles are generated by
                // `scripts/copy-debuginfo.mjs` after `napi build`.
                //   - macOS:   <name>.node.dSYM (directory)
                //   - Windows: <name>.node.pdb  (file)
                //   - Linux:   no extra file; DWARF is in the .node already.
                const dsymSrc = path.join(sourceDir, `${nodeFile}.dSYM`);
                if (fs.existsSync(dsymSrc)) {
                    fs.cpSync(
                        dsymSrc,
                        path.join(targetDir, `${nodeFile}.dSYM`),
                        { recursive: true, dereference: true },
                    );
                }
                const pdbSrc = path.join(sourceDir, `${nodeFile}.pdb`);
                if (fs.existsSync(pdbSrc)) {
                    fs.copyFileSync(
                        pdbSrc,
                        path.join(targetDir, `${nodeFile}.pdb`),
                    );
                }
            }

            // macOS: stage the headless Syphon companion into the app bundle.
            // buildPath is Contents/Resources/app, so Contents is two levels up.
            // The helper goes in Contents/Resources (NOT Contents/MacOS): a second
            // Mach-O sitting beside the main executable is treated as bundle code
            // that codesign requires already-signed when it seals the main binary,
            // and @electron/osx-sign signs same-depth files in directory order — so
            // whenever it reached Operator before operator-syphon the seal failed
            // ("code object is not signed at all"), the failure was swallowed by
            // packager's continueOnError, and notarization then rejected the unsigned
            // app. In Resources the helper is sealed as a resource and signed as its
            // own nested code, with no ordering constraint. @rpath stays
            // @executable_path/../Frameworks, which resolves to Contents/Frameworks
            // from Resources just as it did from MacOS.
            // @electron/osx-sign runs after this hook and signs the helper +
            // Syphon.framework (inside-out, hardened runtime, our Developer ID),
            // so library validation passes without extra entitlements.
            if (platform === 'darwin') {
                const bridgeDist = path.join(
                    __dirname,
                    'native',
                    'syphon-bridge',
                    'dist',
                );
                const helperSrc = path.join(
                    bridgeDist,
                    'MacOS',
                    'operator-syphon',
                );
                const frameworkSrc = path.join(
                    bridgeDist,
                    'Frameworks',
                    'Syphon.framework',
                );
                if (fs.existsSync(helperSrc) && fs.existsSync(frameworkSrc)) {
                    const contents = path.resolve(buildPath, '..', '..');
                    const frameworksDir = path.join(contents, 'Frameworks');
                    fs.mkdirSync(frameworksDir, { recursive: true });
                    // ditto preserves the executable bit, symlinks, and the
                    // framework's Versions structure.
                    const helperDest = path.join(
                        contents,
                        'Resources',
                        'operator-syphon',
                    );
                    execFileSync('ditto', [helperSrc, helperDest]);
                    execFileSync('ditto', [
                        frameworkSrc,
                        path.join(frameworksDir, 'Syphon.framework'),
                    ]);

                    // Fail loudly if the helper doesn't cover every arch the app
                    // ships; otherwise the missing slice silently fails to exec on
                    // that arch. The main app binary is still named "Electron" at
                    // this hook (renamed to Operator later), so derive the target
                    // archs from the `arch` argument instead of reading it. Fix:
                    // build universal (yarn build-syphon-bridge --require --universal).
                    const requiredArchs =
                        arch === 'universal'
                            ? ['arm64', 'x86_64']
                            : arch === 'x64'
                              ? ['x86_64']
                              : [arch];
                    const helperArchs = execFileSync('lipo', [
                        '-archs',
                        helperDest,
                    ])
                        .toString()
                        .trim()
                        .split(/\s+/);
                    const missing = requiredArchs.filter(
                        (a) => !helperArchs.includes(a),
                    );
                    if (missing.length > 0) {
                        throw new Error(
                            `[forge] Syphon helper is missing arch(es) ${missing.join(', ')} for a ${arch} build (helper=[${helperArchs.join(', ')}]). Rebuild universal: yarn build-syphon-bridge --require --universal`,
                        );
                    }
                    console.log(
                        '[forge] staged Syphon companion (operator-syphon + Syphon.framework)',
                    );
                } else if (process.env.SYPHON_SKIP === '1') {
                    console.warn(
                        '[forge] SYPHON_SKIP=1 — packaging without the Syphon helper.',
                    );
                } else {
                    // Don't silently ship a release whose Syphon menu item is dead.
                    throw new Error(
                        '[forge] native/syphon-bridge/dist not found — run `yarn build-syphon-bridge` (needs the Xcode Metal Toolchain) before packaging, or set SYPHON_SKIP=1 to ship without Syphon.',
                    );
                }
            }
        },
    },
    makers: [
        new MakerSquirrel({
            name: 'Operator',
        }),
        new MakerZIP({}, ['darwin']),
        new MakerRpm({
            options: {
                bin: 'Operator',
            },
        }),
        new MakerDeb({
            options: {
                bin: 'Operator',
            },
        }),
        // new MakerFlatpak({
        //   // @ts-ignore
        //   options: {
        //     bin: 'Operator',
        //     id: 'com.helveticascenario.operator',
        //   },
        // }),
    ],
    publishers: [
        new PublisherGithub({
            repository: {
                owner: 'HelveticaScenario',
                name: 'operator',
            },
            prerelease: false,
            draft: false,
        }),
    ],
    plugins: [
        new AutoUnpackNativesPlugin({}),
        new VitePlugin({
            // `build` can specify multiple entry points for the main process
            build: [
                {
                    entry: 'src/main/main.ts',
                    config: 'vite.main.config.ts',
                    target: 'main',
                },
                {
                    entry: 'src/preload/preload.ts',
                    config: 'vite.preload.config.ts',
                    target: 'preload',
                },
            ],
            renderer: [
                {
                    name: 'main_window',
                    config: 'vite.renderer.config.ts',
                },
            ],
        }),
        // Fuses are used to enable/disable various Electron functionality
        // at package time, before code signing the application
        new FusesPlugin({
            version: FuseVersion.V1,
            [FuseV1Options.RunAsNode]: false,
            [FuseV1Options.EnableCookieEncryption]: true,
            [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
            [FuseV1Options.EnableNodeCliInspectArguments]: false,
            [FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true,
            [FuseV1Options.OnlyLoadAppFromAsar]: true,
        }),
    ],
};

export default config;
