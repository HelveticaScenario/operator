import { beforeAll, describe, expect, it, vi } from 'vitest';

import type { EqPanelDef } from '../../shared/dsl/eqPanelTypes';
import type { CodeStyle } from '../dsl/objectPropertyInsert';
import { addSection, changeSection, replayChanges } from './eqPanelOps';
import type { RawEqParams } from './eqPanelModel';
import {
    type EqPanelContext,
    NOTICES,
    type PanelRuntime,
    previousModuleId,
} from './eqPanelRuntime';
import type { EqPanelSetup } from './eqPanelSetup';
import { SpectrumAnalyzer } from './eqSpectrum';

const def: EqPanelDef = {
    key: 'eq:$eq-1',
    moduleId: '$eq-1',
    spectrum: {
        channels: [0],
        moduleId: '$eq-1',
        msPerFrame: 10,
        portName: 'output',
    },
};

function makePanel(params: RawEqParams): PanelRuntime {
    return {
        callbacks: {} as PanelRuntime['callbacks'],
        changeLog: new Map(),
        channel: 0,
        codeTimer: null,
        def,
        engineFrame: null,
        frameListeners: new Set(),
        ghosts: new Map(),
        index: 0,
        lastCodeWrite: 0,
        live: null,
        modulated: false,
        notice: null,
        params,
        pendingCode: new Map(),
        selected: null,
        spectrum: new SpectrumAnalyzer(),
        spectrumKeys: [],
        structureSeq: 0,
        zone: null,
    };
}

function makeCtx(setup: Partial<EqPanelSetup> | null = null) {
    let seq = 0;
    const sent: unknown[] = [];
    const ctx: EqPanelContext = {
        codeStyle: { quote: "'" } as CodeStyle,
        editor: null,
        nextEditSeq: () => ++seq,
        render: vi.fn(),
        setModuleParam: (_id, _type, params) => {
            sent.push(structuredClone(params));
            return Promise.resolve();
        },
        setup: setup as EqPanelSetup | null,
    };
    return { ctx, sent };
}

const oneBand = (): RawEqParams => ({
    input: 0,
    sections: [{ freq: '1000hz', gain: 3, type: 'peak' }],
});

beforeAll(() => {
    // Engine sends coalesce on animation frames; a microtask stands in.
    vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
        queueMicrotask(cb);
        return 1;
    });
    vi.stubGlobal('cancelAnimationFrame', () => {});
});

describe('previousModuleId', () => {
    const setup = (remap: Record<string, string>) =>
        ({ moduleIdRemap: remap }) as EqPanelSetup;

    it('follows the engine instance across a renumbering', () => {
        expect(previousModuleId(setup({ '$eq-1': '$eq-2' }), '$eq-2')).toBe(
            '$eq-1',
        );
    });

    it('treats an id now naming a different instance as new', () => {
        expect(
            previousModuleId(setup({ '$eq-1': '$eq-2' }), '$eq-1'),
        ).toBeNull();
    });

    it('keeps an unchanged id', () => {
        expect(previousModuleId(setup({}), '$eq-1')).toBe('$eq-1');
    });
});

describe('panel changes', () => {
    it('logs each section change for a queued run to replay', () => {
        const { ctx } = makeCtx();
        const panel = makePanel(oneBand());
        changeSection(ctx, panel, 0, { gainDb: 6 }, 'move', false);
        changeSection(ctx, panel, 0, { freqHz: 2000 }, 'commit', false);
        const [entry] = [...panel.changeLog.values()];
        expect(panel.changeLog.size).toBe(1);
        expect(entry).toMatchObject({
            change: { freqHz: 2000, gainDb: 6 },
            codeOnly: false,
            index: 0,
            list: 0,
        });
    });

    it('replays logged changes onto a queued run’s params and sends them', () => {
        const { ctx, sent } = makeCtx();
        const panel = makePanel(oneBand());
        changeSection(ctx, panel, 0, { gainDb: 6 }, 'commit', false);
        const changes = [...panel.changeLog.values()];
        // The queued run was compiled before the change.
        panel.params = oneBand();
        sent.length = 0;
        replayChanges(ctx, panel, changes);
        expect(panel.params.sections[0]).toMatchObject({ gain: 6 });
        expect(sent).toHaveLength(1);
    });

    it('replays code-only changes as ghosts, not engine values', () => {
        const { ctx, sent } = makeCtx();
        const panel = makePanel(oneBand());
        replayChanges(ctx, panel, [
            {
                change: { gainDb: 9 },
                codeOnly: true,
                index: 0,
                list: 0,
                seq: 1,
            },
        ]);
        expect(panel.params.sections[0]).toMatchObject({ gain: 3 });
        expect(panel.ghosts.get('0:0')).toEqual({ gainDb: 9 });
        expect(sent).toHaveLength(0);
    });

    it('leaves the engine alone when an add cannot reach the code', () => {
        const model = { isDisposed: () => false };
        const { ctx, sent } = makeCtx({
            decorations: { getRange: () => null } as never,
            model: model as never,
        });
        const panel = makePanel(oneBand());
        panel.def = { ...def, sourceLocation: { column: 1, line: 1 } };
        addSection(ctx, panel, 500, 0);
        expect(panel.params.sections).toHaveLength(1);
        expect(sent).toHaveLength(0);
        expect(panel.notice).toBe(NOTICES.missing);
    });

    it('adds live-only when the call cannot be edited', () => {
        const { ctx, sent } = makeCtx();
        const panel = makePanel(oneBand());
        addSection(ctx, panel, 500, 0);
        expect(panel.params.sections).toHaveLength(2);
        expect(sent).toHaveLength(1);
        expect(panel.structureSeq).toBeGreaterThan(0);
    });
});
