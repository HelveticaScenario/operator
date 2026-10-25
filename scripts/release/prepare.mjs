#!/usr/bin/env node

/**
 * Turns the pending changie fragments into the next release: writes
 * `.changes/vX.Y.Z.md`, regenerates CHANGELOG.md and sets package.json's
 * version. The Release PR workflow runs this on a fresh branch off master;
 * merging that PR is what ships the release (see .github/workflows/release.yml).
 *
 * The version comes from the fragments' kinds (`changie next auto`) unless
 * `.changes/release-as` names one explicitly; that file is consumed here.
 *
 * Prints nothing and changes nothing when there are no pending fragments.
 * Otherwise writes `version=X.Y.Z` to $GITHUB_OUTPUT (or stdout).
 */

import { execFileSync } from 'node:child_process';
import {
    appendFileSync,
    existsSync,
    readdirSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const CHANGES = join(ROOT, '.changes');
const RELEASE_AS = join(CHANGES, 'release-as');
const PACKAGE_JSON = join(ROOT, 'package.json');

const changie = (...args) =>
    execFileSync(join(ROOT, 'node_modules', '.bin', 'changie'), args, {
        cwd: ROOT,
        encoding: 'utf8',
    }).trim();

const pending = readdirSync(join(CHANGES, 'unreleased')).filter((name) =>
    name.endsWith('.yaml'),
);
if (pending.length === 0) {
    process.exit(0);
}

const tag = existsSync(RELEASE_AS)
    ? readFileSync(RELEASE_AS, 'utf8').trim()
    : changie('next', 'auto');
if (!/^v\d+\.\d+\.\d+$/.test(tag)) {
    throw new Error(`Release version must look like v1.2.3, got "${tag}"`);
}

changie('batch', tag);
changie('merge');
rmSync(RELEASE_AS, { force: true });

const version = tag.slice(1);
const pkg = readFileSync(PACKAGE_JSON, 'utf8');
writeFileSync(
    PACKAGE_JSON,
    pkg.replace(/^(\s*"version":\s*")[^"]*(")/m, `$1${version}$2`),
);

const output = `version=${version}\n`;
if (process.env.GITHUB_OUTPUT) {
    appendFileSync(process.env.GITHUB_OUTPUT, output);
} else {
    process.stdout.write(output);
}
