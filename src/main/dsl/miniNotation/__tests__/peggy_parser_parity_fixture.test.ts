/**
 * Emits the Peggy↔Rust parser parity fixture consumed by the Rust
 * integration test `crates/modular_core/tests/peggy_parser_parity.rs`.
 *
 * For every input case, parse with the production Peggy TS parser, dump
 * the AST as JSON, and store both. The Rust descent test_parser then
 * parses the same input, serializes its AST through serde, and the
 * fixture is compared after a tiny "normalization" pass that zeroes
 * RandomChoice / Degrade seeds (their depth-first counter assignment is
 * equivalent but not bit-identical across the two parsers in all nested
 * cases — the seed value itself is incidental for shape parity).
 *
 * Note: this file does NOT consume the fixture; it produces it. The
 * `peggy_parser_parity_matches_rust_descent` test below is the
 * cross-validation step on the TS side (re-parses each input and asserts
 * the emitted JSON round-trips). The Rust test is the real parity gate.
 *
 * Run via `yarn gen:parser-parity-fixture` to refresh after grammar
 * changes.
 */

import { describe, expect, test } from 'vitest';
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { parseMini } from '../parser';
import type { MiniAST } from '../ast';

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURE_PATH = resolve(
    __dirname,
    '..',
    '..',
    '..',
    '..',
    '..',
    'crates',
    'modular_core',
    'src',
    'pattern_system',
    '__fixtures__',
    'peggy_parser_parity.json',
);

// Grammar surface covered by the Rust descent test_parser. The TS Peggy
// parser supports a strict superset (e.g. `-` as Rest, polymeter `{...}`,
// `$p()`-only sigils) — fixture rows are restricted to the intersection
// so the Rust side can actually parse each input.
//
// Mirrors GRAMMAR_CASES in `sp.test.ts`, minus:
//   - "rest dash" (`-`)  — test_parser.rs treats `-` only as a number prefix.
const PARITY_CASES: Array<{ label: string; input: string }> = [
    { label: 'pure number', input: '0' },
    { label: 'pure negative number', input: '-3' },
    { label: 'pure Hz', input: '440hz' },
    { label: 'note with octave', input: 'c4' },
    { label: 'note bare letter (octave null)', input: 'c' },
    { label: 'note sharp', input: 'c#4' },
    { label: 'note flat', input: 'eb3' },
    { label: 'note flat no octave', input: 'eb' },
    { label: 'note b-flat no octave', input: 'bb' },
    { label: 'note f-alias flat no octave', input: 'cf' },
    { label: 'note s-alias sharp', input: 'cs4' },
    { label: 'rest tilde', input: '~' },
    { label: 'sequence (null weights)', input: '0 1 2' },
    { label: 'fast subsequence []', input: '[0 1] 2' },
    { label: 'slow subsequence <>', input: '<0 1 2>' },
    { label: 'stack via comma', input: '0,1,2' },
    { label: 'nested stack inside []', input: '[0 1, 2 3]' },
    { label: 'fast modifier *n', input: '0*2' },
    { label: 'slow modifier /n', input: '0/2' },
    { label: 'replicate !n', input: '0!3' },
    { label: 'replicate ! default', input: '0!' },
    { label: 'degrade ? with prob', input: '0?0.5' },
    { label: 'degrade ? default prob (null)', input: '0?' },
    { label: 'euclidean (k,n) no rotation', input: '0(3,8)' },
    { label: 'euclidean with rotation', input: '0(3,8,1)' },
    { label: 'fast factor as subsequence', input: 'c*[1 2]' },
    { label: 'slow factor as fast subsequence', input: '0/[2 3]' },
    { label: 'fast factor as slow subsequence', input: '0*<2 3>' },
    { label: 'slow factor as slow subsequence', input: '0/<2 3>' },
    { label: 'weight @n positional', input: '0@2 1' },
    { label: 'random choice |', input: '0|1|2' },
    { label: 'rest inside choice', input: '0|~|2' },
    { label: 'choice of space-separated sequences', input: '0 1 | 2 3' },
    { label: 'choice of fast subsequences', input: '[0 1] | [2 3]' },
    {
        label: 'choice of comma-chord subsequences',
        input: '[0,0,0] | [0,-7,0]',
    },
    { label: 'replicate !! accumulates to 3', input: '0!!' },
    { label: 'weight bare @ defaults to 2', input: '0@' },
];

/**
 * Walk a MiniAST JSON value and zero every RandomChoice / Degrade seed.
 * Seeds are assigned depth-first on both sides but the depth-first
 * walk order can differ for nested constructs (the Peggy action fires
 * top-down, the descent parser increments bottom-up). The seed itself
 * is determinism metadata for downstream `Pattern` construction — its
 * exact numeric value is not part of the syntactic shape.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function normalizeSeeds(node: any): any {
    if (Array.isArray(node)) {
        return node.map(normalizeSeeds);
    }
    if (node === null || typeof node !== 'object') {
        return node;
    }
    if ('RandomChoice' in node) {
        // Shape: { RandomChoice: [children[], seed] }
        const [children] = node.RandomChoice;
        return { RandomChoice: [normalizeSeeds(children), 0] };
    }
    if ('Degrade' in node) {
        // Shape: { Degrade: [child, prob|null, seed] }
        const [child, prob] = node.Degrade;
        return { Degrade: [normalizeSeeds(child), prob, 0] };
    }
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(node)) {
        out[k] = normalizeSeeds(v);
    }
    return out;
}

describe('peggy parser parity fixture', () => {
    test('emits canonical fixture for Rust cross-parser comparison', () => {
        const rows = PARITY_CASES.map(({ label, input }) => {
            const peggyAst = parseMini(input) as MiniAST;
            const normalized = normalizeSeeds(peggyAst);
            return { label, input, peggy_ast: normalized };
        });
        mkdirSync(dirname(FIXTURE_PATH), { recursive: true });
        writeFileSync(FIXTURE_PATH, JSON.stringify(rows, null, 2));
        // Round-trip self-check: every emitted AST must reparse to the
        // same normalized shape (catches non-deterministic re-runs at
        // the source).
        for (const { input, peggy_ast } of rows) {
            const re = normalizeSeeds(parseMini(input) as MiniAST);
            expect(re).toEqual(peggy_ast);
        }
        // Sanity on case-count so the fixture stays a meaningful sample.
        expect(rows.length).toBeGreaterThanOrEqual(20);
    });
});
