/**
 * Validates that every code example embedded in the generated DSL type
 * definitions evaluates to a valid patch.
 *
 * The generated `dsl.d.ts` (produced by `buildLibSource`) is the documentation
 * surface developers read in Monaco. Its examples come from two sources:
 *   1. `@example` JSDoc tags hand-written in `typescriptLibGen.ts`.
 *   2. ```js fenced blocks inside each module's Rust doc comment, carried
 *      through `schemas.json` into the generated JSDoc.
 *
 * This test regenerates that surface, extracts every example, and asserts each
 * one runs through `executePatchScript` without error and produces a graph that
 * passes the engine's own `validatePatchGraph`. A failure here means a code
 * example a user could copy out of the editor does not actually work — fix it
 * at its source (the `@example` tag or the Rust doc comment).
 */

import { describe, expect, test } from 'vitest';
import { validatePatchGraph } from '@modular/core';
import schemas from '@modular/core/schemas.json';
import { buildLibSource } from '../typescriptLibGen';
import {
    type DSLExecutionOptions,
    type WavsFolderNode,
    executePatchScript,
} from '../executor';

// ─── Execution environment ──────────────────────────────────────────────────
//
// A stub wavs folder so examples that read samples via `$wavs()` can build a
// graph without real audio files on disk. Covers every wav path referenced by
// the examples (`pad`, `strings`, `kick`, `tables/pad`).

const STUB_WAVS_TREE: WavsFolderNode = {
    pad: 'file',
    strings: 'file',
    kick: 'file',
    tables: { pad: 'file' },
};

const stubLoadWav: NonNullable<DSLExecutionOptions['loadWav']> = (path) => ({
    channels: 2,
    frameCount: 96_000,
    path,
    sampleRate: 48_000,
    duration: 2,
    bitDepth: 16,
    loops: [],
    cuePoints: [],
    mtime: 0,
});

const EXECUTION_OPTIONS: DSLExecutionOptions = {
    sampleRate: 48_000,
    workspaceRoot: '/workspace',
    wavsFolderTree: STUB_WAVS_TREE,
    loadWav: stubLoadWav,
};

// ─── Example extraction ─────────────────────────────────────────────────────

interface DocExample {
    /** Where the example came from: an `@example` tag or a ```js fence. */
    kind: 'tag' | 'fence';
    /** Ordinal within the generated source, for stable test labels. */
    ordinal: number;
    /** The example source code. */
    code: string;
    /** A short, human-readable label for the test name. */
    label: string;
}

/** Strip the leading ` * ` JSDoc gutter from every line of a comment block. */
function stripJsDocGutter(block: string): string {
    return block
        .split('\n')
        .map((line) => line.replace(/^[ \t]*\*?[ \t]?/, ''))
        .join('\n');
}

const FENCE_RE = /```(?:js|javascript|ts|typescript)?[ \t]*\r?\n([\s\S]*?)```/g;

/** Collapse an example into a one-line label (first non-comment line). */
function makeLabel(code: string): string {
    const firstCode =
        code
            .split('\n')
            .map((l) => l.trim())
            .find((l) => l.length > 0 && !l.startsWith('//')) ?? code.trim();
    return firstCode.length > 72 ? `${firstCode.slice(0, 69)}...` : firstCode;
}

/**
 * Extract every code example from a generated `dsl.d.ts` string: `@example`
 * tags and ```js fenced blocks living inside JSDoc comments.
 */
export function extractDocExamples(generated: string): DocExample[] {
    const examples: DocExample[] = [];
    let ordinal = 0;

    const jsDocBlocks = generated.matchAll(/\/\*\*([\s\S]*?)\*\//g);
    for (const blockMatch of jsDocBlocks) {
        const clean = stripJsDocGutter(blockMatch[1]);

        // Fenced code blocks (module documentation carried from Rust).
        let withoutFences = clean;
        for (const fence of clean.matchAll(FENCE_RE)) {
            const code = fence[1].replace(/\s+$/, '');
            if (code.trim().length > 0) {
                examples.push({
                    kind: 'fence',
                    ordinal: ordinal++,
                    code,
                    label: makeLabel(code),
                });
            }
        }
        // Remove fences so `@example` parsing below can't see code inside them.
        withoutFences = clean.replace(FENCE_RE, '');

        // `@example` tags. An example body is the text on the tag line plus any
        // following lines up to the next block tag (`@param`, `@see`, …) or EOF.
        const lines = withoutFences.split('\n');
        for (let i = 0; i < lines.length; i++) {
            // Tolerate leftover indentation after the gutter strip so an
            // extra-indented `@example` is never silently dropped.
            const tag = lines[i].match(/^[ \t]*@example[ \t]?(.*)$/);
            if (!tag) continue;
            const body: string[] = [];
            if (tag[1].trim().length > 0) body.push(tag[1]);
            for (let j = i + 1; j < lines.length; j++) {
                if (/^[ \t]*@\w+/.test(lines[j])) break;
                body.push(lines[j]);
                i = j;
            }
            const code = body.join('\n').replace(/\s+$/, '');
            if (code.trim().length > 0) {
                examples.push({
                    kind: 'tag',
                    ordinal: ordinal++,
                    code,
                    label: makeLabel(code),
                });
            }
        }
    }

    return examples;
}

/**
 * Some examples document TypeScript *types*, not patches (e.g.
 * `type T = ElementsOf<[number[], string[]]>`). They can never be a patch, so
 * they are excluded from patch validation.
 */
function isTypeLevelExample(code: string): boolean {
    return /^\s*(export\s+)?(type|interface|declare)\b/.test(code);
}

// ─── The check ──────────────────────────────────────────────────────────────

const generated = buildLibSource(schemas);
const allExamples = extractDocExamples(generated);
const patchExamples = allExamples.filter((e) => !isTypeLevelExample(e.code));

describe('generated DSL example extraction', () => {
    test('finds a substantial number of examples', () => {
        // Guards against the extractor silently matching nothing.
        expect(allExamples.length).toBeGreaterThan(80);
        expect(patchExamples.length).toBeGreaterThan(80);
    });
});

describe('every generated code example evaluates to a valid patch', () => {
    test.each(patchExamples)('[$kind #$ordinal] $label', ({ code }) => {
        let result: ReturnType<typeof executePatchScript>;
        try {
            result = executePatchScript(code, schemas, EXECUTION_OPTIONS);
        } catch (error) {
            throw new Error(
                `Example failed to execute:\n\n${code}\n\n→ ${
                    error instanceof Error ? error.message : String(error)
                }`,
                { cause: error },
            );
        }

        const errors = validatePatchGraph(result.patch);
        if (errors.length > 0) {
            const detail = errors
                .map((e) => `  • ${e.field}: ${e.message}`)
                .join('\n');
            throw new Error(
                `Example built an invalid patch:\n\n${code}\n\n${detail}`,
            );
        }
    });
});
