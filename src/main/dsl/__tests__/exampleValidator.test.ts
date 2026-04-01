/**
 * Example validator — ensures every DSL code example in schemas.json and
 * typeDocs.ts can be parsed and executed without errors.
 *
 * Run with: yarn test:unit
 *
 * This test extracts fenced ```js code blocks from module documentation
 * (schemas.json) and all example strings from typeDocs.ts, then feeds each
 * through executePatchScript(). If an API change breaks an example, this
 * test will catch it.
 */

import { describe, test, expect } from 'vitest';
import schemas from '@modular/core/schemas.json';
import { executePatchScript } from '../executor';
import { TYPE_DOCS, GLOBAL_DOCS } from '../../../shared/dsl/typeDocs';

// ─── Helpers ──────────────────────────────────────────────────────────────────

/**
 * Extract fenced ```js code blocks from a markdown-style documentation string.
 * Returns an array of code strings (without the fence markers).
 */
function extractJsCodeBlocks(documentation: string): string[] {
    const blocks: string[] = [];
    const regex = /```js\r?\n([\s\S]*?)```/g;
    let match: RegExpExecArray | null;
    while ((match = regex.exec(documentation)) !== null) {
        const code = match[1].trim();
        if (code.length > 0) {
            blocks.push(code);
        }
    }
    return blocks;
}

/**
 * Heuristic: returns true if the string looks like executable DSL code
 * (starts with a $ function call, `const`, or contains `.out()`).
 * Pure value literals like '"C4"' or '"440hz"' are not executable.
 */
function isExecutableDSL(example: string): boolean {
    const trimmed = example.trim();
    // Skip pure string/number literals used as type illustrations
    if (/^(['"].*['"]|[\d.]+)$/.test(trimmed)) return false;
    // Skip comment-only examples (all non-empty lines are comments)
    const lines = trimmed.split('\n').filter((l) => l.trim());
    if (lines.length > 0 && lines.every((l) => l.trim().startsWith('//'))) {
        return false;
    }
    // Must contain a DSL function call, assignment, or method chain
    return /(\$\w+|^\s*(const|let|var)\s+|\.\w+\()/.test(trimmed);
}

/**
 * Run a DSL example through executePatchScript and assert it doesn't throw.
 */
function assertExecutable(source: string) {
    expect(() => executePatchScript(source, schemas)).not.toThrow();
}

// ─── schemas.json documentation examples ─────────────────────────────────────

describe('schemas.json documentation examples', () => {
    for (const schema of schemas) {
        const doc = (schema as { documentation?: string }).documentation;
        if (!doc) continue;

        const codeBlocks = extractJsCodeBlocks(doc);
        if (codeBlocks.length === 0) continue;

        describe(schema.name, () => {
            codeBlocks.forEach((code, i) => {
                const label =
                    codeBlocks.length === 1
                        ? 'example'
                        : `example ${i + 1}`;
                test(label, () => {
                    assertExecutable(code);
                });
            });
        });
    }
});

// ─── typeDocs.ts TYPE_DOCS examples ──────────────────────────────────────────

describe('typeDocs TYPE_DOCS examples', () => {
    for (const [typeName, typeDoc] of Object.entries(TYPE_DOCS)) {
        const allExamples: Array<{ label: string; code: string }> = [];

        // Top-level examples
        typeDoc.examples.forEach((ex, i) => {
            allExamples.push({
                label: `${typeName} example ${i + 1}`,
                code: ex,
            });
        });

        // Method examples
        if (typeDoc.methods) {
            for (const method of typeDoc.methods) {
                if (method.example) {
                    allExamples.push({
                        label: `${typeName}.${method.name}()`,
                        code: method.example,
                    });
                }
            }
        }

        const executable = allExamples.filter((e) => isExecutableDSL(e.code));
        if (executable.length === 0) continue;

        describe(typeName, () => {
            for (const { label, code } of executable) {
                test(label, () => {
                    assertExecutable(code);
                });
            }
        });
    }
});

// ─── typeDocs.ts GLOBAL_DOCS examples ────────────────────────────────────────

describe('typeDocs GLOBAL_DOCS examples', () => {
    for (const fnDoc of GLOBAL_DOCS) {
        const executable = fnDoc.examples.filter(isExecutableDSL);
        if (executable.length === 0) continue;

        describe(fnDoc.name, () => {
            executable.forEach((code, i) => {
                const label =
                    executable.length === 1
                        ? 'example'
                        : `example ${i + 1}`;
                test(label, () => {
                    assertExecutable(code);
                });
            });
        });
    }
});
