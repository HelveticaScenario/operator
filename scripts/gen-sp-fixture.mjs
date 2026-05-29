// Strudel ground-truth hap generator for $sp chainable combination tests.
//
// Two fixtures emitted:
//
//   sp_combine.json
//     Single-op chain. (lhs, rhs, op, mode) cross product over the
//     integer-only grammar surface — 10 × 10 × 2 × 7 = 1400 rows.
//
//   sp_combine_chain2.json
//     Two-op chain. (lhs, rhs1, rhs2, op1, mode1, op2, mode2) over a
//     reduced grammar surface — 4 × 4 × 4 × 2 × 7 × 2 × 7 = 12 544 rows.
//     Mirrors $sp(...).add(rhs1).add(rhs2) (or .sub) folds.
//
// Run from modular repo root:
//   node scripts/gen-sp-fixture.mjs
//
// Requires @strudel/core + @strudel/mini installed as devDeps. They
// are pinned at 1.2.5 — the 1.2.6 release added a transitive
// @kabelsalat/web dependency whose published `dist/index.mjs` is
// missing the `SalatRepl` named export that strudel's repl.mjs imports,
// breaking package-main resolution.

import * as mini from '@strudel/mini';
import * as core from '@strudel/core';
import { writeFile, mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURES = resolve(
    __dirname,
    '..',
    'crates',
    'modular_core',
    'src',
    'pattern_system',
    '__fixtures__',
);

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

// Reduced surface used for the 2-op cartesian — picked to cover
// distinct structural shapes (single atom, multi-atom sequence,
// time-modifier, gap-bearing). 4³ = 64 grammar tuples → 12544 rows.
const CHAIN_CASES = [
    { label: 'atom', source: '0' },
    { label: 'sequence_2', source: '0 1' },
    { label: 'fast_2', source: '0*2' },
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

function applyOp(pat, rhsPat, opName, modeName) {
    return pat[MODE_METHOD[modeName]](rhsPat, OPS[opName]);
}

function queryRow(combined) {
    const haps = combined.queryArc(0, 1);
    return haps.map(dumpHap);
}

function genSingleChain() {
    const rows = [];
    for (const lhs of CASES) {
        const lhsPat = mini.mini(lhs.source);
        for (const rhs of CASES) {
            const rhsPat = mini.mini(rhs.source);
            for (const opName of Object.keys(OPS)) {
                for (const modeName of MODES) {
                    let haps;
                    try {
                        haps = queryRow(applyOp(lhsPat, rhsPat, opName, modeName));
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
                        haps,
                    });
                }
            }
        }
    }
    return rows;
}

function genChain2() {
    const rows = [];
    for (const lhs of CHAIN_CASES) {
        const lhsPat = mini.mini(lhs.source);
        for (const rhs1 of CHAIN_CASES) {
            const rhs1Pat = mini.mini(rhs1.source);
            for (const rhs2 of CHAIN_CASES) {
                const rhs2Pat = mini.mini(rhs2.source);
                for (const op1 of Object.keys(OPS)) {
                    for (const mode1 of MODES) {
                        let firstPat;
                        try {
                            firstPat = applyOp(lhsPat, rhs1Pat, op1, mode1);
                        } catch (err) {
                            rows.push({
                                lhs: lhs.label,
                                lhs_source: lhs.source,
                                rhs1: rhs1.label,
                                rhs1_source: rhs1.source,
                                rhs2: rhs2.label,
                                rhs2_source: rhs2.source,
                                op1,
                                mode1,
                                op2: null,
                                mode2: null,
                                error: `step1: ${err.message ?? err}`,
                            });
                            continue;
                        }
                        for (const op2 of Object.keys(OPS)) {
                            for (const mode2 of MODES) {
                                let haps;
                                try {
                                    haps = queryRow(
                                        applyOp(firstPat, rhs2Pat, op2, mode2),
                                    );
                                } catch (err) {
                                    rows.push({
                                        lhs: lhs.label,
                                        lhs_source: lhs.source,
                                        rhs1: rhs1.label,
                                        rhs1_source: rhs1.source,
                                        rhs2: rhs2.label,
                                        rhs2_source: rhs2.source,
                                        op1,
                                        mode1,
                                        op2,
                                        mode2,
                                        error: `step2: ${err.message ?? err}`,
                                    });
                                    continue;
                                }
                                rows.push({
                                    lhs: lhs.label,
                                    lhs_source: lhs.source,
                                    rhs1: rhs1.label,
                                    rhs1_source: rhs1.source,
                                    rhs2: rhs2.label,
                                    rhs2_source: rhs2.source,
                                    op1,
                                    mode1,
                                    op2,
                                    mode2,
                                    haps,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    return rows;
}

await mkdir(FIXTURES, { recursive: true });

const single = genSingleChain();
const singlePath = resolve(FIXTURES, 'sp_combine.json');
await writeFile(singlePath, JSON.stringify(single, null, 2));
console.log(
    `wrote ${single.length} single-chain rows (${single.filter((r) => r.haps).length} ok, ${single.filter((r) => r.error).length} errors) → ${singlePath}`,
);

const chain2 = genChain2();
const chain2Path = resolve(FIXTURES, 'sp_combine_chain2.json');
await writeFile(chain2Path, JSON.stringify(chain2, null, 2));
console.log(
    `wrote ${chain2.length} chain-2 rows (${chain2.filter((r) => r.haps).length} ok, ${chain2.filter((r) => r.error).length} errors) → ${chain2Path}`,
);
