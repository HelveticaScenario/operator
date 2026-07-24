#!/usr/bin/env node
// Generate the electron-updater `latest-mac.yml` from the macOS zip(s) that
// `electron-forge make` writes under `out/make`. electron-updater's macOS
// provider resolves an update by reading this file from the release feed, so it
// must ship as a GitLab Release asset alongside the zip (see .gitlab-ci.yml).
//
// Windows needs no equivalent: Squirrel.Windows reads its own RELEASES manifest,
// which the maker already emits. Linux has no in-app installer. On those hosts
// this script finds no darwin zip and exits without writing anything.
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, writeFileSync, statSync } from 'node:fs';
import { dirname, join, basename } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = dirname(here);
const makeDir = join(repoRoot, 'out', 'make');

const { version } = JSON.parse(
    readFileSync(join(repoRoot, 'package.json'), 'utf8'),
);

// Recursively collect every darwin zip the maker produced (one per arch).
function findDarwinZips(dir) {
    const out = [];
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
            out.push(...findDarwinZips(full));
        } else if (entry.name.endsWith('.zip') && entry.name.includes('darwin')) {
            out.push(full);
        }
    }
    return out;
}

let zips = [];
try {
    zips = findDarwinZips(makeDir);
} catch {
    // out/make is absent on a host that ran no `make`; nothing to do.
}

if (zips.length === 0) {
    console.log('[update-metadata] no darwin zip found — skipping latest-mac.yml');
    process.exit(0);
}

const files = zips.map((zip) => {
    const buf = readFileSync(zip);
    return {
        url: basename(zip),
        sha512: createHash('sha512').update(buf).digest('base64'),
        size: statSync(zip).size,
    };
});

// The top-level path/sha512 are the primary artifact for clients that ignore
// the per-arch `files` list; prefer the arch this build runs on.
const primary =
    files.find((f) => f.url.includes(process.arch)) ?? files[0];

const lines = [
    `version: ${version}`,
    'files:',
    ...files.flatMap((f) => [
        `  - url: ${f.url}`,
        `    sha512: '${f.sha512}'`,
        `    size: ${f.size}`,
    ]),
    `path: ${primary.url}`,
    `sha512: '${primary.sha512}'`,
    `releaseDate: '${new Date().toISOString()}'`,
    '',
];

const dest = join(makeDir, 'latest-mac.yml');
writeFileSync(dest, lines.join('\n'));
console.log(
    `[update-metadata] wrote ${dest} for ${files.map((f) => f.url).join(', ')}`,
);
