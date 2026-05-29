// Strudel ground-truth hap generator for $sp chainable combination tests.
//
// For every (lhs, rhs) pair drawn from a representative integer-only
// mini-notation grammar surface, for every op ∈ {add, sub}, for every
// mode ∈ {in, out, mix, squeeze, squeezeout, reset, restart}, query the
// first cycle of strudel's combined pattern and dump the haps. The
// emitted JSON becomes the fixture against which Operator's combine_sp
// Rust impl is regression-tested.
//
// Run via:
//   cp scripts/gen-sp-fixture.mjs ~/dev/audio/strudel/
//   cd ~/dev/audio/strudel && pnpm install
//   node gen-sp-fixture.mjs
//   rm gen-sp-fixture.mjs
//
// (The `@strudel/mini` package only resolves when the script lives
// inside the strudel workspace — pnpm-linked deps don't resolve from
// outside.)
//
// Output goes to:
//   crates/modular_core/src/pattern_system/__fixtures__/sp_combine.json

import * as mini from '@strudel/mini';
import * as core from '@strudel/core';
import { writeFile, mkdir } from 'node:fs/promises';
import { dirname } from 'node:path';

// Integer-only grammar surface — every case parses to a Pattern<number>
// in both strudel and Operator (after IntervalValue mapping). Rest-bearing
// cases are intentionally excluded because strudel propagates `undefined`
// where Operator propagates IntervalValue::Rest; the impls match
// structurally but not at the JSON value level.
const CASES = [
    { label: 'atom', source: '0' },
    { label: 'sequence_2', source: '0 1' },
    { label: 'sequence_3', source: '0 1 2' },
    { label: 'group_inner', source: '[0 1] 2' },
    { label: 'stack', source: '0,1' },
    { label: 'slow_cat', source: '<0 1 2>' },
    { label: 'fast_2', source: '0*2' },
    { label: 'slow_2', source: '0/2' },
    { label: 'replicate_3', source: '0!3' },
    { label: 'euclidean_3_8', source: '0(3,8)' },
];

const OPS = {
    add: (a) => (b) => a + b,
    sub: (a) => (b) => a - b,
};

const MODES = [
    'in',
    'out',
    'mix',
    'squeeze',
    'squeezeout',
    'reset',
    'restart',
];

const MODE_METHOD = {
    in: '_opIn',
    out: '_opOut',
    mix: '_opMix',
    squeeze: '_opSqueeze',
    squeezeout: '_opSqueezeOut',
    reset: '_opReset',
    restart: '_opRestart',
};

function fracTuple(f) {
    if (f === undefined || f === null) return null;
    // BigInt n/d -> number (all denominators here are small)
    return [Number(f.s * f.n), Number(f.d)];
}

function dumpHap(h) {
    return {
        whole: h.whole
            ? [fracTuple(h.whole.begin), fracTuple(h.whole.end)]
            : null,
        part: [fracTuple(h.part.begin), fracTuple(h.part.end)],
        value: h.value,
    };
}

function applyCombine(lhsPat, rhsPat, opName, modeName) {
    const method = MODE_METHOD[modeName];
    const f = OPS[opName];
    return lhsPat[method](rhsPat, f);
}

const rows = [];

for (const lhs of CASES) {
    const lhsPat = mini.mini(lhs.source);
    for (const rhs of CASES) {
        const rhsPat = mini.mini(rhs.source);
        for (const opName of Object.keys(OPS)) {
            for (const modeName of MODES) {
                let haps;
                try {
                    const combined = applyCombine(
                        lhsPat,
                        rhsPat,
                        opName,
                        modeName,
                    );
                    haps = combined.queryArc(0, 1);
                } catch (err) {
                    rows.push({
                        lhs: lhs.label,
                        lhs_source: lhs.source,
                        rhs: rhs.label,
                        rhs_source: rhs.source,
                        op: opName,
                        mode: modeName,
                        error: String(err.message ?? err),
                    });
                    continue;
                }
                rows.push({
                    lhs: lhs.label,
                    lhs_source: lhs.source,
                    rhs: rhs.label,
                    rhs_source: rhs.source,
                    op: opName,
                    mode: modeName,
                    haps: haps.map(dumpHap),
                });
            }
        }
    }
}

// Absolute path so the script works when copied into the strudel
// workspace (see header for the workflow).
const outPath = '/Users/helveticascenario/dev/modular.claude-bold-johnson-132a38/crates/modular_core/src/pattern_system/__fixtures__/sp_combine.json';
await mkdir(dirname(outPath), { recursive: true });
await writeFile(outPath, JSON.stringify(rows, null, 2));
console.log(
    `wrote ${rows.length} rows (${rows.filter((r) => r.haps).length} ok, ${rows.filter((r) => r.error).length} errors) → ${outPath}`,
);
