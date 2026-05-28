/**
 * Integration tests for the N-API boundary (@modular/core).
 *
 * These run against the compiled native module — no Electron or audio device needed.
 */

import { describe, test, expect } from 'vitest';
import {
    validatePatchGraph,
    deriveChannelCount,
    type PatchGraph,
} from '@modular/core';

// ─── validatePatchGraph ──────────────────────────────────────────────────────

describe('validatePatchGraph', () => {
    test('empty patch is valid', () => {
        const patch: PatchGraph = { modules: [], scopes: [] };
        const errors = validatePatchGraph(patch);
        expect(errors).toEqual([]);
    });

    test('valid single-module patch with correct param name passes', () => {
        const patch: PatchGraph = {
            modules: [
                {
                    id: 'sine-1',
                    moduleType: '$sine',
                    idIsExplicit: false,
                    params: { freq: '440hz' },
                },
            ],
            scopes: [],
        };
        const errors = validatePatchGraph(patch);
        expect(errors).toEqual([]);
    });

    test('wrong param name "frequency" is rejected via deserialization', () => {
        // Unknown param validation is now handled by deserr (deny_unknown_fields)
        // during deserialization, not by validatePatchGraph.
        const result = deriveChannelCount('$sine', { frequency: '440hz' });
        expect(result.channelCount).toBeUndefined();
        expect(result.errors).toBeDefined();
        expect(result.errors!.length).toBeGreaterThan(0);
        expect(result.errors![0].message).toMatch(/unknown parameter/i);
    });

    test('invalid module type produces errors', () => {
        const patch: PatchGraph = {
            modules: [
                {
                    id: 'bad-1',
                    moduleType: '$nonExistentFooBar',
                    idIsExplicit: false,
                    params: {},
                },
            ],
            scopes: [],
        };
        const errors = validatePatchGraph(patch);
        expect(errors.length).toBeGreaterThan(0);
        expect(errors[0].message).toMatch(/unknown|not found|exist/i);
    });

    test('invalid param name produces errors via deserialization', () => {
        // Unknown param validation is now handled by deserr (deny_unknown_fields)
        // during deserialization, not by validatePatchGraph.
        const result = deriveChannelCount('$sine', {
            freq: '440hz',
            bogusParam: 42,
        });
        expect(result.channelCount).toBeUndefined();
        expect(result.errors).toBeDefined();
        expect(result.errors!.length).toBeGreaterThan(0);
        expect(result.errors![0].message).toMatch(/unknown parameter/i);
    });

    test('scope referencing non-existent module produces errors', () => {
        const patch: PatchGraph = {
            modules: [],
            scopes: [
                {
                    channels: [
                        {
                            moduleId: 'ghost-module',
                            portName: 'output',
                            channel: 0,
                        },
                    ],
                    msPerFrame: 50,
                    range: [-5, 5] as [number, number],
                },
            ],
        };
        const errors = validatePatchGraph(patch);
        expect(errors.length).toBeGreaterThan(0);
    });

    test('cable referencing non-existent module produces errors', () => {
        const patch: PatchGraph = {
            modules: [
                {
                    id: 'lpf-1',
                    moduleType: '$lpf',
                    idIsExplicit: false,
                    params: {
                        input: {
                            type: 'cable',
                            module: 'does-not-exist',
                            port: 'output',
                            channel: 0,
                        },
                        cutoff: '1000hz',
                    },
                },
            ],
            scopes: [],
        };
        const errors = validatePatchGraph(patch);
        expect(errors.length).toBeGreaterThan(0);
    });

    test('error objects have field and message', () => {
        const patch: PatchGraph = {
            modules: [
                {
                    id: 'bad-1',
                    moduleType: '$nonExistentModule',
                    idIsExplicit: false,
                    params: {},
                },
            ],
            scopes: [],
        };
        const errors = validatePatchGraph(patch);
        expect(errors.length).toBeGreaterThan(0);
        expect(errors[0]).toHaveProperty('field');
        expect(errors[0]).toHaveProperty('message');
        expect(typeof errors[0].field).toBe('string');
        expect(typeof errors[0].message).toBe('string');
    });
});

// ─── deriveChannelCount ──────────────────────────────────────────────────────

describe('deriveChannelCount', () => {
    test('single note returns 1', () => {
        const result = deriveChannelCount('$sine', { freq: 'C4' });
        expect(result.channelCount).toBe(1);
        expect(result.errors).toBeUndefined();
    });

    test('array of notes returns correct count', () => {
        const result = deriveChannelCount('$sine', {
            freq: ['C4', 'E4', 'G4'],
        });
        expect(result.channelCount).toBe(3);
        expect(result.errors).toBeUndefined();
    });

    test('unknown module type returns errors', () => {
        const result = deriveChannelCount('$unknownFoo', { x: 1 });
        expect(result.channelCount).toBeUndefined();
        expect(result.errors).toBeDefined();
        expect(result.errors!.length).toBeGreaterThan(0);
    });

    test('Hz string returns 1', () => {
        const result = deriveChannelCount('$sine', { freq: '440hz' });
        expect(result.channelCount).toBe(1);
        expect(result.errors).toBeUndefined();
    });

    test('missing required param returns error with param name', () => {
        const result = deriveChannelCount('$lpf', { cutoff: 'C4' });
        expect(result.channelCount).toBeUndefined();
        expect(result.errors).toBeDefined();
        expect(result.errors![0].params).toContain('input');
    });
});

// `getMiniLeafSpans` and `getPatternPolyphony` were removed when
// mini-notation parsing moved TypeScript-side (`$p()`). The TS parser is
// covered by `src/main/dsl/miniNotation/__tests__/parser.test.ts`;
// pre-cached polyphony derivation is exercised through the seq/iCycle
// deserialization tests in `crates/modular_core/src/dsp/seq/`.
