// Prints a markdown section listing every module marked
// `#[module(..., experimental)]`, with how long since its source file last
// changed. Prints nothing when no module is experimental. The release PR
// workflow embeds stdout verbatim, so the script writes nothing else there.
//
//   node scripts/release/experimental-report.mjs

import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const MODULE_SRC = join(ROOT, 'crates/modular_core/src');
const OVERDUE_DAYS = 60;
const DAY_MS = 24 * 60 * 60 * 1000;

function rustFiles(dir) {
    return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) {
            return rustFiles(path);
        }
        return entry.name.endsWith('.rs') ? [path] : [];
    });
}

/** The argument text of each `#[module(...)]` attribute in `source`. */
function moduleAttributes(source) {
    const attrs = [];
    const opener = '#[module(';
    let start = source.indexOf(opener);
    while (start !== -1) {
        let depth = 1;
        let inString = false;
        let i = start + opener.length;
        for (; i < source.length && depth > 0; i++) {
            const c = source[i];
            if (inString) {
                if (c === '\\') {
                    i++;
                } else if (c === '"') {
                    inString = false;
                }
            } else if (c === '"') {
                inString = true;
            } else if (c === '(') {
                depth++;
            } else if (c === ')') {
                depth--;
            }
        }
        attrs.push(source.slice(start + opener.length, i - 1));
        start = source.indexOf(opener, i);
    }
    return attrs;
}

function experimentalModules() {
    const modules = [];
    for (const file of rustFiles(MODULE_SRC)) {
        for (const attr of moduleAttributes(readFileSync(file, 'utf8'))) {
            // Drop string literals and nested argument lists, leaving the
            // top-level flags and `key = value` pairs.
            let flags = attr.replace(/"(?:[^"\\]|\\.)*"/g, '""');
            while (/\([^()]*\)/.test(flags)) {
                flags = flags.replace(/\([^()]*\)/g, '');
            }
            const isExperimental = flags
                .split(',')
                .some((part) => part.trim() === 'experimental');
            const name = attr.match(/\bname\s*=\s*"((?:[^"\\]|\\.)*)"/)?.[1];
            if (isExperimental && name) {
                modules.push({ name, file: relative(ROOT, file) });
            }
        }
    }
    return modules;
}

/** Committer date (YYYY-MM-DD) of the last commit touching `file`, if any. */
function lastChanged(file) {
    const out = execFileSync('git', ['log', '-1', '--format=%cs', '--', file], {
        cwd: ROOT,
        encoding: 'utf8',
    });
    return out.trim() || undefined;
}

// `%cs` is the committer's local calendar date, so compare against today's
// local date.
const now = new Date();
const today = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
const rows = experimentalModules().map((m) => {
    const date = lastChanged(m.file);
    const days = date ? Math.round((today - Date.parse(date)) / DAY_MS) : 0;
    return {
        ...m,
        date: date ?? 'uncommitted',
        days,
        overdue: days > OVERDUE_DAYS,
    };
});

if (rows.length > 0) {
    rows.sort(
        (a, b) =>
            Number(b.overdue) - Number(a.overdue) ||
            b.days - a.days ||
            a.name.localeCompare(b.name),
    );
    const lines = [
        '## Experimental modules',
        '',
        `Modules unchanged for more than ${OVERDUE_DAYS} days are **overdue**: ` +
            'decide whether to graduate, change, or remove them.',
        '',
        '| Module | Source | Last changed | Days | Status |',
        '| --- | --- | --- | --- | --- |',
        ...rows.map(
            (r) =>
                `| \`${r.name}\` | \`${r.file}\` | ${r.date} | ${r.days} | ` +
                `${r.overdue ? '**overdue**' : ''} |`,
        ),
    ];
    process.stdout.write(lines.join('\n') + '\n');
}
