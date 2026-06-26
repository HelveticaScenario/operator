#!/usr/bin/env node
/**
 * Resolve unreleased migration markers to the version being released.
 *
 * A migration is authored with `sinceVersion: 'next'` because the author can't
 * know which release will ship it. `release.sh` runs this after bumping the
 * version so every `'next'` marker becomes the exact release version, stamping
 * each migration with the release it ships in — no manual bookkeeping.
 *
 * Usage: node scripts/resolveMigrationVersions.mjs <version>
 * Prints the paths it changed (one per line) so the caller can `git add` them.
 */
import { globSync, readFileSync, writeFileSync } from 'node:fs';

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
    console.error(
        `resolveMigrationVersions: expected a semver argument, got ${JSON.stringify(version)}`,
    );
    process.exit(1);
}

const MARKER = /sinceVersion:\s*'next'/g;
const changed = [];

for (const file of globSync('src/renderer/dsl/**/*.ts')) {
    const source = readFileSync(file, 'utf-8');
    if (!MARKER.test(source)) continue;
    writeFileSync(file, source.replace(MARKER, `sinceVersion: '${version}'`));
    changed.push(file);
}

for (const file of changed) console.log(file);
