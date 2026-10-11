/**
 * Tests for `ui: true` EQ panel emission: the panel defs beside the patch,
 * their source anchors, and the headless spectrum scopes inside it.
 */

import { describe, expect, test } from 'vitest';
import schemas from '@modular/core/schemas.json';
import { FIRST_LINE_COLUMN_OFFSET } from '../../../shared/dsl/spanTypes';
import { EQ_SPECTRUM_MS_PER_FRAME } from '../../../shared/dsl/eqPanelTypes';
import type { ScopeWithLocation } from '../GraphBuilder';
import { type DSLExecutionResult, executePatchScript } from '../executor';

function exec(source: string): DSLExecutionResult {
    return executePatchScript(source, schemas, {
        sampleRate: 48_000,
        workspaceRoot: '/workspace',
    });
}

/** Text starting at a captured (V8, 1-based) source location. */
function textAt(source: string, loc: { line: number; column: number }) {
    const line = source.split('\n')[loc.line - 1];
    const column =
        loc.line === 1 ? loc.column - FIRST_LINE_COLUMN_OFFSET : loc.column;
    return line.slice(column - 1);
}

describe('eq panels', () => {
    test('ui: true emits a panel anchored at the eq method name', () => {
        const source = `const sig = $saw('c2')
$unstable.eq(sig, {
    sections: [{ type: 'lowShelf', freq: '150hz', gain: 3 }],
    ui: true,
}).out()
$unstable.eq(sig, { sections: [{ freq: 0 }] }).out()`;
        const result = exec(source);
        expect(result.eqPanels).toHaveLength(1);
        const [panel] = result.eqPanels;
        expect(panel.key).toBe(`eq:${panel.moduleId}`);
        expect(panel.sourceLocation).toEqual(panel.zoneLocation);
        expect(textAt(source, panel.sourceLocation!)).toMatch(/^eq\(sig, \{$/);

        const module = result.patch.modules.find(
            (m) => m.id === panel.moduleId,
        )!;
        expect(module.moduleType).toBe('$unstable.eq');
        expect(module.params.ui).toBe(true);

        const key =
            `${panel.zoneLocation!.line}:${panel.zoneLocation!.column}` as const;
        expect(result.callSiteSpans.get(key)).toEqual({
            endLine: 5,
            startLine: 2,
        });
    });

    test('anchors on line 1 too', () => {
        const source = `$unstable.eq($saw('c2'), { ui: true }).out()`;
        const [panel] = exec(source).eqPanels;
        expect(textAt(source, panel.sourceLocation!)).toMatch(/^eq\(/);
    });

    test('the $ chain form anchors at its eq method name', () => {
        const source = `$saw('c2').$.unstable.eq({ ui: true }).out()`;
        const [panel] = exec(source).eqPanels;
        expect(textAt(source, panel.sourceLocation!)).toMatch(/^eq\(\{/);
    });

    test('a shared list gets a headless full-rate scope on channel 0', () => {
        const result = exec(
            `$unstable.eq($saw(['c2', 'c3']), { sections: [{ freq: 0 }], ui: true }).out()`,
        );
        const [panel] = result.eqPanels;
        const feeds = (result.patch.scopes as ScopeWithLocation[]).filter(
            (s) => s.headless,
        );
        expect(feeds).toHaveLength(1);
        expect(feeds[0]).toMatchObject({
            channels: [
                { channel: 0, moduleId: panel.moduleId, portName: 'output' },
            ],
            msPerFrame: EQ_SPECTRUM_MS_PER_FRAME,
        });
        expect(panel.spectrum).toEqual({
            channels: [0],
            moduleId: panel.moduleId,
            msPerFrame: EQ_SPECTRUM_MS_PER_FRAME,
            portName: 'output',
        });
    });

    test('per-channel lists get one feed per list channel', () => {
        const result = exec(`$unstable.eq($saw('c2'), {
            sections: [[{ freq: 0 }], [{ freq: 1 }]],
            ui: true,
        }).out()`);
        const [panel] = result.eqPanels;
        expect(panel.spectrum.channels).toEqual([0, 1]);
        const feeds = (result.patch.scopes as ScopeWithLocation[]).filter(
            (s) => s.headless,
        );
        expect(feeds.map((f) => f.channels[0].channel)).toEqual([0, 1]);
    });

    test('eqs sharing a call site keep zone placement but are not editable', () => {
        const result = exec(`
            const band = (s) => $unstable.eq(s, { ui: true })
            band($saw('c2')).out()
            band($saw('c3')).out()
        `);
        expect(result.eqPanels).toHaveLength(2);
        for (const panel of result.eqPanels) {
            expect(panel.sourceLocation).toBeUndefined();
            expect(panel.zoneLocation).toBeDefined();
        }
    });

    test('no ui, no panels or feeds', () => {
        const result = exec(
            `$unstable.eq($saw('c2'), { sections: [{ freq: 0 }] }).out()`,
        );
        expect(result.eqPanels).toEqual([]);
        expect(
            (result.patch.scopes as ScopeWithLocation[]).some(
                (s) => s.headless,
            ),
        ).toBe(false);
    });
});
