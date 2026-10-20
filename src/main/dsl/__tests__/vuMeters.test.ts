/**
 * Tests for the VU meter output stage: `.out()` / `.outMono()` label, mute,
 * and solo options, the compiled gate chain (__vuTap → __vuSlew → gate
 * $scaleAndShift driven by a __vuMute $signal), and the `patch.vuMeters`
 * entries the renderer panel consumes.
 */

import { describe, expect, test } from 'vitest';
import type { ModuleSpec, PatchGraph } from '@modular/core';
import schemas from '@modular/core/schemas.json';
import type { VuMeterDef } from '../../../shared/dsl/vuMeterTypes';
import { type DSLExecutionResult, executePatchScript } from '../executor';

const DEFAULT_EXECUTION_OPTIONS = {
    sampleRate: 48_000,
    workspaceRoot: '/workspace',
} as const;

function exec(source: string): DSLExecutionResult {
    return executePatchScript(source, schemas, DEFAULT_EXECUTION_OPTIONS);
}

function execPatch(source: string): PatchGraph {
    return exec(source).patch;
}

function vuMeters(patch: PatchGraph): VuMeterDef[] {
    return patch.vuMeters as unknown as VuMeterDef[];
}

function moduleById(patch: PatchGraph, id: string): ModuleSpec {
    const module = patch.modules.find((m) => m.id === id);
    expect(module, `module ${id} should exist`).toBeDefined();
    return module!;
}

describe('vuMeters emission', () => {
    test('labeled stereo and mono outs emit specs in source order', () => {
        const patch = execPatch(`
            $sine('c4').out({ label: 'lead', baseChannel: 2 })
            $saw('c2').outMono(0, { label: 'bass' })
        `);
        const meters = vuMeters(patch);
        expect(meters).toHaveLength(3);

        expect(meters[0]).toMatchObject({
            baseChannel: 2,
            channels: 2,
            key: 'lead',
            label: 'lead',
            moduleId: '__vuTap_lead',
            mute: false,
            muteModuleId: '__vuMute_lead',
            portName: 'output',
            solo: false,
        });
        expect(meters[1]).toMatchObject({
            baseChannel: 0,
            channels: 1,
            key: 'bass',
            label: 'bass',
            moduleId: '__vuTap_bass',
            muteModuleId: '__vuMute_bass',
        });
        // Source order wins even though compilation is channel-sorted (the
        // bass group compiles first as channel 0 < 2); the end-of-chain
        // master meter always comes last.
        expect(meters.map((m) => m.key)).toEqual(['lead', 'bass', '__main__']);
    });

    test('the end-of-chain master meter taps ROOT_OUTPUT', () => {
        const patch = execPatch(`$sine('c4').out()`);
        const meters = vuMeters(patch);
        const main = meters[meters.length - 1];
        expect(main).toMatchObject({
            channels: 2,
            gain: 2.5,
            gainLocked: false,
            gainModuleId: '__vuGain_main',
            key: '__main__',
            label: 'Main',
            main: true,
            moduleId: 'ROOT_OUTPUT',
            pan: null,
            panModuleId: null,
            portName: 'output',
        });
        expect(main.muteModuleId).toBeUndefined();
        // The lifted master gain seeds the builder's output-gain default.
        expect(moduleById(patch, '__vuGain_main').params.value).toBe(2.5);
    });

    test('$setOutputGain seeds the lifted master gain', () => {
        const patch = execPatch(`
            $setOutputGain(4)
            $sine('c4').out()
        `);
        expect(moduleById(patch, '__vuGain_main').params.value).toBe(4);
        const meters = vuMeters(patch);
        expect(meters[meters.length - 1].gain).toBe(4);
    });

    test('a signal-valued output gain locks the master fader', () => {
        const patch = execPatch(`
            $setOutputGain($sine('0.1hz').range(0, 5))
            $sine('c4').out()
        `);
        const meters = vuMeters(patch);
        const main = meters[meters.length - 1];
        expect(main.gain).toBeNull();
        expect(main.gainModuleId).toBeNull();
        expect(main.gainLocked).toBe(true);
        expect(main.gainSource).toBeDefined();
        expect(patch.modules.some((m) => m.id === '__vuGain_main')).toBe(false);
    });

    test('unlabeled outs get positional keys and matching ids', () => {
        const patch = execPatch(`
            $sine('c4').out()
            $saw('c2').outMono()
        `);
        const meters = vuMeters(patch);
        expect(meters.map((m) => m.key)).toEqual([
            'out 1',
            'out 2',
            '__main__',
        ]);
        expect(meters[0].moduleId).toBe('__vuTap_out_1');
        expect(meters[0].muteModuleId).toBe('__vuMute_out_1');
        expect(meters[1].moduleId).toBe('__vuTap_out_2');
        // Positional ids carry no identity across edits, so the hidden
        // modules are flagged implicit for the similarity reconciler;
        // label-derived ids stay explicit.
        expect(moduleById(patch, '__vuTap_out_1').idIsExplicit).toBe(false);
        expect(moduleById(patch, '__vuMute_out_2').idIsExplicit).toBe(false);
    });

    test('only unlabeled outs start their gain and pan fresh on a swap', () => {
        const patch = execPatch(`
            $sine('c4').out()
            $saw('c2').out({ label: 'bass' })
        `);
        // A structural match could pair an unlabeled out's controls with
        // another out's, so they skip state transfer; label-derived ids
        // match their own predecessor and keep their ramp.
        expect(moduleById(patch, '__vuGain_out_1').skipStateTransfer).toBe(
            true,
        );
        expect(moduleById(patch, '__vuPan_out_1').skipStateTransfer).toBe(true);
        expect(
            moduleById(patch, '__vuGain_bass').skipStateTransfer,
        ).toBeUndefined();
        expect(
            moduleById(patch, '__vuGain_main').skipStateTransfer,
        ).toBeUndefined();
    });

    test('a label claiming a positional name shifts auto keys past it', () => {
        const patch = execPatch(`
            $sine('c4').out({ label: 'out 1' })
            $saw('c2').out()
        `);
        const meters = vuMeters(patch).filter((m) => !m.main);
        expect(meters.map((m) => m.key)).toEqual(['out 1', 'out 2']);
        expect(meters.map((m) => m.moduleId)).toEqual([
            '__vuTap_out_1',
            '__vuTap_out_2',
        ]);
        expect(moduleById(patch, '__vuTap_out_1').idIsExplicit).toBe(true);
        expect(moduleById(patch, '__vuTap_out_2').idIsExplicit).toBe(false);
    });

    test('outs sharing one call site carry no sourceLocation', () => {
        const patch = execPatch(`for (const n of ['c3', 'g3']) $sine(n).out()`);
        const meters = vuMeters(patch).filter((m) => !m.main);
        expect(meters).toHaveLength(2);
        expect(meters[0].sourceLocation).toBeUndefined();
        expect(meters[1].sourceLocation).toBeUndefined();
    });

    test('labels are sanitized into module ids', () => {
        const patch = execPatch(`$sine('c4').out({ label: 'my lead!' })`);
        const meters = vuMeters(patch);
        expect(meters[0].key).toBe('my lead!');
        expect(meters[0].moduleId).toBe('__vuTap_my_lead_');
    });

    test('no outs produce an empty vuMeters list', () => {
        const patch = execPatch(`$sine('c4')`);
        expect(vuMeters(patch)).toEqual([]);
        const root = moduleById(patch, 'ROOT_OUTPUT');
        expect(root.moduleType).toBe('$signal');
    });

    test('source locations and call site spans cover out calls', () => {
        const result = exec(`$sine('c4').out({ label: 'lead' })`);
        const meters = vuMeters(result.patch);
        expect(meters[0].sourceLocation).toBeDefined();
        const { line, column } = meters[0].sourceLocation!;
        expect(result.callSiteSpans.get(`${line}:${column}`)).toBeDefined();
    });

    test('collection out produces one group', () => {
        const patch = execPatch(
            `$saw(['c3', 'e3', 'g3']).out({ label: 'chord' })`,
        );
        const meters = vuMeters(patch).filter((m) => !m.main);
        expect(meters).toHaveLength(1);
        expect(meters[0].channels).toBe(2);
    });
});

describe('pan lifting', () => {
    test('stereo out with no pan lifts a centered pan _smoothControl', () => {
        const patch = execPatch(`$sine('c4').out({ label: 'lead' })`);
        const pan = moduleById(patch, '__vuPan_lead');
        expect(pan.moduleType).toBe('_smoothControl');
        expect(pan.params.value).toBe(0);
        const meter = vuMeters(patch)[0];
        expect(meter.pan).toBe(0);
        expect(meter.panModuleId).toBe('__vuPan_lead');
        // The stereo mixer's pan input is cabled from the lifted signal.
        const mixers = patch.modules.filter(
            (m) =>
                m.moduleType === '$stereoMix' &&
                JSON.stringify(m.params.pan ?? '').includes('__vuPan_lead'),
        );
        expect(mixers).toHaveLength(1);
    });

    test('a numeric pan option seeds the lifted signal', () => {
        const patch = execPatch(
            `$sine('c4').out({ label: 'lead', pan: -2.5 })`,
        );
        expect(moduleById(patch, '__vuPan_lead').params.value).toBe(-2.5);
        expect(vuMeters(patch)[0].pan).toBe(-2.5);
    });

    test('a signal-valued pan renders locked, with a live tap', () => {
        const patch = execPatch(
            `$sine('c4').out({ label: 'lead', pan: $sine('1hz') })`,
        );
        expect(patch.modules.some((m) => m.id === '__vuPan_lead')).toBe(false);
        const meter = vuMeters(patch)[0];
        expect(meter.pan).toBeNull();
        expect(meter.panModuleId).toBeNull();
        expect(meter.panLocked).toBe(true);
        expect(meter.panSource).toBeDefined();
        expect(meter.panSource!.portName).toBe('output');
    });

    test('editable pans carry no lock or tap', () => {
        const patch = execPatch(`$sine('c4').out({ label: 'lead' })`);
        const meter = vuMeters(patch)[0];
        expect(meter.panLocked).toBe(false);
        expect(meter.panSource).toBeUndefined();
    });

    test('mono outs have no pan control', () => {
        const patch = execPatch(`$saw('c2').outMono(0, { label: 'bass' })`);
        const meter = vuMeters(patch)[0];
        expect(meter.pan).toBeNull();
        expect(meter.panModuleId).toBeNull();
        expect(patch.modules.some((m) => m.id.startsWith('__vuPan_'))).toBe(
            false,
        );
    });
});

describe('gain lifting', () => {
    test('an absent gain lifts a unity gain _smoothControl', () => {
        const patch = execPatch(`$sine('c4').out({ label: 'lead' })`);
        const gain = moduleById(patch, '__vuGain_lead');
        expect(gain.moduleType).toBe('_smoothControl');
        expect(gain.params.value).toBe(5);
        const meter = vuMeters(patch)[0];
        expect(meter.gain).toBe(5);
        expect(meter.gainModuleId).toBe('__vuGain_lead');
        // The lifted gain feeds a $curve; the tap is the gain stage itself.
        const curves = patch.modules.filter(
            (m) =>
                m.moduleType === '$curve' &&
                JSON.stringify(m.params.input ?? '').includes('__vuGain_lead'),
        );
        expect(curves).toHaveLength(1);
        expect(moduleById(patch, '__vuTap_lead').moduleType).toBe(
            '$scaleAndShift',
        );
    });

    test('a numeric gain option seeds the lifted signal', () => {
        const patch = execPatch(
            `$saw('c2').outMono(0, { gain: 2.5, label: 'bass' })`,
        );
        expect(moduleById(patch, '__vuGain_bass').params.value).toBe(2.5);
        expect(vuMeters(patch)[0].gain).toBe(2.5);
    });

    test('a positional numeric outMono gain seeds the lifted signal', () => {
        const patch = execPatch(`$saw('c2').outMono(0, 2.5)`);
        expect(moduleById(patch, '__vuGain_out_1').params.value).toBe(2.5);
    });

    test('a signal-valued gain renders locked, with a live tap', () => {
        const patch = execPatch(
            `$sine('c4').out({ label: 'lead', gain: $sine('1hz').range(0, 5) })`,
        );
        expect(patch.modules.some((m) => m.id === '__vuGain_lead')).toBe(false);
        const meter = vuMeters(patch)[0];
        expect(meter.gain).toBeNull();
        expect(meter.gainModuleId).toBeNull();
        expect(meter.gainLocked).toBe(true);
        expect(meter.gainSource).toBeDefined();
        // The gain stage still compiles and carries the tap.
        expect(moduleById(patch, '__vuTap_lead').moduleType).toBe(
            '$scaleAndShift',
        );
    });
});

describe('label validation', () => {
    test('duplicate labels throw', () => {
        expect(() =>
            execPatch(`
                $sine('c4').out({ label: 'a' })
                $saw('c2').outMono(0, { label: 'a' })
            `),
        ).toThrow(/must be unique/);
    });

    test('labels colliding after sanitization get distinct ids', () => {
        const patch = execPatch(`
            $sine('c4').out({ label: 'a b' })
            $saw('c2').out({ label: 'a_b' })
        `);
        const meters = vuMeters(patch).filter((m) => !m.main);
        expect(meters.map((m) => m.key)).toEqual(['a b', 'a_b']);
        expect(meters.map((m) => m.moduleId)).toEqual([
            '__vuTap_a_b',
            '__vuTap_a_b_2',
        ]);
    });

    test('the reserved __main__ label throws', () => {
        expect(() =>
            execPatch(`$sine('c4').out({ label: '__main__' })`),
        ).toThrow(/reserved/);
    });

    test("a 'main' label never collides with the master gain id", () => {
        const patch = execPatch(`$sine('c4').out({ label: 'main' })`);
        expect(vuMeters(patch)[0].moduleId).toBe('__vuTap_main_2');
        expect(moduleById(patch, '__vuGain_main').moduleType).toBe(
            '_smoothControl',
        );
    });

    test('empty label throws', () => {
        expect(() => execPatch(`$sine('c4').out({ label: '' })`)).toThrow(
            /non-empty/,
        );
    });
});

describe('output channel bounds', () => {
    test('a mono out reaches the engine’s last channel', () => {
        const patch = execPatch(`$sine('c4').outMono(63)`);
        const source = moduleById(patch, 'ROOT_OUTPUT').params
            .value as unknown[];
        expect(source).toHaveLength(64);
        expect(vuMeters(patch)[0].baseChannel).toBe(63);
    });

    test('a stereo out reaches the last pair', () => {
        const patch = execPatch(`$sine('c4').out({ baseChannel: 62 })`);
        const source = moduleById(patch, 'ROOT_OUTPUT').params
            .value as unknown[];
        expect(source).toHaveLength(64);
    });

    test('channels past the engine width throw', () => {
        expect(() => execPatch(`$sine('c4').outMono(64)`)).toThrow(
            'channel must be 0-63',
        );
        // A stereo pair needs its right channel to fit too.
        expect(() => execPatch(`$sine('c4').out({ baseChannel: 63 })`)).toThrow(
            'baseChannel must be 0-62',
        );
    });
});

describe('outMono second-argument overload', () => {
    test('a numeric second argument is a gain', () => {
        const patch = execPatch(`$saw('c2').outMono(0, 2.5)`);
        // A gain adds a $curve into the output chain.
        expect(patch.modules.some((m) => m.moduleType === '$curve')).toBe(true);
        // The tap id sits on the gain $scaleAndShift.
        expect(moduleById(patch, '__vuTap_out_1').moduleType).toBe(
            '$scaleAndShift',
        );
    });

    test('an options-only argument can carry the channel', () => {
        const patch = execPatch(
            `$saw('c2').outMono({ channel: 2, label: 'bass' })`,
        );
        const meter = vuMeters(patch)[0];
        expect(meter.baseChannel).toBe(2);
        expect(meter.label).toBe('bass');
    });

    test('an options channel wins over the positional argument', () => {
        const patch = execPatch(`$saw('c2').outMono(0, { channel: 3 })`);
        expect(vuMeters(patch)[0].baseChannel).toBe(3);
    });

    test('an options object second argument carries gain and label', () => {
        const patch = execPatch(
            `$saw('c2').outMono(0, { gain: 2.5, label: 'bass' })`,
        );
        expect(patch.modules.some((m) => m.moduleType === '$curve')).toBe(true);
        expect(vuMeters(patch)[0].label).toBe('bass');
        expect(moduleById(patch, '__vuTap_bass').moduleType).toBe(
            '$scaleAndShift',
        );
    });

    test('a signal second argument is a gain', () => {
        const patch = execPatch(`$saw('c2').outMono(0, $sine('1hz'))`);
        expect(patch.modules.some((m) => m.moduleType === '$curve')).toBe(true);
    });
});

describe('gate chain compilation', () => {
    test('the tap id always sits on the gain stage', () => {
        const patch = execPatch(`$sine('c4').out({ label: 'lead' })`);
        expect(moduleById(patch, '__vuTap_lead').moduleType).toBe(
            '$scaleAndShift',
        );
    });

    test('gate chain: $signal → $slew → $scaleAndShift fed by the tap', () => {
        const patch = execPatch(`$sine('c4').out({ label: 'lead' })`);

        const mute = moduleById(patch, '__vuMute_lead');
        expect(mute.moduleType).toBe('$signal');
        expect(mute.params.value).toBe(5);

        const slew = moduleById(patch, '__vuSlew_lead');
        expect(slew.moduleType).toBe('$slew');
        expect(slew.params.rise).toBe(0.002);
        expect(slew.params.fall).toBe(0.002);
        expect(slew.params.input).toMatchObject([
            { module: '__vuMute_lead', type: 'cable' },
        ]);
        // The gate must sit at this patch's mute state from a swap's first
        // sample; carrying the old slew level across would leak audio
        // through a still-closing gate.
        expect(slew.skipStateTransfer).toBe(true);

        // The gate stage is a $scaleAndShift whose input cables come from the
        // tap and whose scale comes from the slewed gate.
        const gates = patch.modules.filter(
            (m) =>
                m.moduleType === '$scaleAndShift' &&
                JSON.stringify(m.params.scale ?? '').includes('__vuSlew_lead'),
        );
        expect(gates).toHaveLength(1);
        expect(JSON.stringify(gates[0].params.input)).toContain('__vuTap_lead');
    });

    test('mute compiles the gate to 0', () => {
        const patch = execPatch(
            `$sine('c4').out({ label: 'lead', mute: true })`,
        );
        expect(moduleById(patch, '__vuMute_lead').params.value).toBe(0);
        expect(vuMeters(patch)[0].mute).toBe(true);
    });

    test('a solo elsewhere gates non-soloed outputs to 0', () => {
        const patch = execPatch(`
            $sine('c4').out({ label: 'a' })
            $saw('c2').out({ label: 'b', solo: true })
        `);
        expect(moduleById(patch, '__vuMute_a').params.value).toBe(0);
        expect(moduleById(patch, '__vuMute_b').params.value).toBe(5);
    });

    test('solo wins over mute on the same output', () => {
        const patch = execPatch(
            `$sine('c4').out({ label: 'a', mute: true, solo: true })`,
        );
        expect(moduleById(patch, '__vuMute_a').params.value).toBe(5);
    });

    test('mute composes with a user gain (both stages present)', () => {
        const patch = execPatch(
            `$sine('c4').out({ label: 'lead', gain: 2, mute: true })`,
        );
        // Tap = the gain stage; the gate stage reads from it.
        expect(moduleById(patch, '__vuTap_lead').moduleType).toBe(
            '$scaleAndShift',
        );
        expect(moduleById(patch, '__vuMute_lead').params.value).toBe(0);
        expect(patch.modules.some((m) => m.moduleType === '$curve')).toBe(true);
    });

    test('a fresh execution resets ordinals and labels', () => {
        const first = execPatch(`$sine('c4').out({ label: 'a' })`);
        expect(vuMeters(first).filter((m) => !m.main)).toHaveLength(1);
        // The same label compiles again — builder state does not leak
        // between executions.
        const second = execPatch(`$sine('c4').out({ label: 'a' })`);
        expect(vuMeters(second)[0].moduleId).toBe('__vuTap_a');
        const third = execPatch(`$sine('c4').out()`);
        expect(vuMeters(third)[0].key).toBe('out 1');
    });
});

describe('$outLane', () => {
    /** The cable(s) feeding the `probe` $signal's value. */
    function probe(
        patch: PatchGraph,
    ): { channel: number; module: string; port: string }[] {
        const source = moduleById(patch, 'probe').params.value;
        return (Array.isArray(source) ? source : [source]) as {
            channel: number;
            module: string;
            port: string;
        }[];
    }

    test('the lane is its post-gate stereo audio', () => {
        const patch = execPatch(`
            $signal($outLane('bass'), { id: 'probe' })
            $saw('c2').out({ label: 'bass' })
        `);
        expect(probe(patch)).toHaveLength(2);
        const [left, right] = probe(patch);
        expect(left).toMatchObject({ channel: 0 });
        expect(right).toMatchObject({ channel: 1, module: left.module });
        const gate = moduleById(patch, left.module);
        expect(gate.moduleType).toBe('$scaleAndShift');
        expect(gate.params.scale).toMatchObject([{ module: '__vuSlew_bass' }]);
    });

    test('a mono lane fills both channels', () => {
        const patch = execPatch(`
            $saw('c2').outMono(0, { label: 'bass' })
            $signal($outLane('bass'), { id: 'probe' })
        `);
        const [left, right] = probe(patch);
        expect(right).toEqual(left);
    });

    test('.active taps the mute/solo gate', () => {
        const patch = execPatch(`
            $signal($outLane('bass').active, { id: 'probe' })
            $saw('c2').out({ label: 'bass' })
            $sine('c4').out({ label: 'lead', solo: true })
        `);
        expect(probe(patch)[0]).toMatchObject({
            module: '__vuMute_bass',
            port: 'output',
        });
        expect(moduleById(patch, '__vuMute_bass').params.value).toBe(0);
    });

    test('.level and .pan tap the lifted controls', () => {
        const patch = execPatch(`
            const bass = $outLane('bass')
            $saw('c2').out({ label: 'bass', gain: 3, pan: -2 })
            $signal(bass.level, { id: 'probe' })
            $signal(bass.pan, { id: 'panProbe' })
        `);
        expect(probe(patch)[0]).toMatchObject({ module: '__vuGain_bass' });
        expect(moduleById(patch, 'panProbe').params.value).toMatchObject({
            module: '__vuPan_bass',
        });
    });

    test('signal-valued gain and pan are tapped directly', () => {
        const patch = execPatch(`
            const g = $signal(4, { id: 'g' })
            const p = $signal(1, { id: 'p' })
            $saw('c2').out({ label: 'bass', gain: g, pan: p })
            $signal($outLane('bass').level, { id: 'probe' })
            $signal($outLane('bass').pan, { id: 'panProbe' })
        `);
        expect(probe(patch)[0]).toMatchObject({ module: 'g' });
        expect(moduleById(patch, 'panProbe').params.value).toMatchObject({
            module: 'p',
        });
    });

    test('a mono lane pans center', () => {
        const patch = execPatch(`
            $saw('c2').outMono(0, { label: 'bass' })
            $signal($outLane('bass').pan, { id: 'probe' })
        `);
        expect(moduleById(patch, probe(patch)[0].module).params.value).toBe(0);
    });

    /** The constant a `$signal`-fed cable carries. */
    function constantOf(patch: PatchGraph, cable: unknown): unknown {
        return moduleById(patch, (cable as { module: string }).module).params
            .value;
    }

    /** The `$remap` params behind a probe on `x.range(...)`. */
    function remapParams(patch: PatchGraph, id: string) {
        const [cable] = moduleById(patch, id).params.value as {
            module: string;
        }[];
        return moduleById(patch, cable.module).params;
    }

    test('properties declare their default ranges', () => {
        const patch = execPatch(`
            const bass = $outLane('bass')
            $saw('c2').out({ label: 'bass' })
            $signal(bass.active.range(-1, 1), { id: 'active' })
            $signal(bass.level.range(-1, 1), { id: 'level' })
            $signal(bass.pan.range(-1, 1), { id: 'pan' })
        `);
        expect(remapParams(patch, 'active')).toMatchObject({
            inMax: 5,
            inMin: 0,
        });
        const level = remapParams(patch, 'level');
        expect(constantOf(patch, level.inMin)).toBe(0);
        expect(constantOf(patch, level.inMax)).toBe(10);
        const pan = remapParams(patch, 'pan');
        expect(constantOf(patch, pan.inMin)).toBe(-5);
        expect(constantOf(patch, pan.inMax)).toBe(5);
    });

    test('a ranged gain signal lends .level its range', () => {
        const patch = execPatch(`
            $saw('c2').out({ label: 'bass', gain: $sine('1hz').range(1, 4) })
            $signal($outLane('bass').level.range(-1, 1), { id: 'level' })
        `);
        const level = remapParams(patch, 'level');
        expect(constantOf(patch, level.inMin)).toBe(1);
        expect(constantOf(patch, level.inMax)).toBe(4);
    });

    test('a pattern gain plays through a single $cycle', () => {
        const patch = execPatch(`
            $saw('c2').out({ label: 'bass', gain: $p('1 4') })
            $signal($outLane('bass').level, { id: 'probe' })
        `);
        const cycles = patch.modules.filter((m) => m.moduleType === '$cycle');
        expect(cycles).toHaveLength(1);
        expect(probe(patch)[0]).toMatchObject({ module: cycles[0].id });
    });

    test('slices keep the lane properties', () => {
        const patch = execPatch(`
            $saw('c2').out({ label: 'bass' })
            const [left] = $outLane('bass').chunk(1)
            $signal(left.active, { id: 'probe' })
            $signal(left, { id: 'audio' })
        `);
        expect(probe(patch)[0]).toMatchObject({ module: '__vuMute_bass' });
        expect(moduleById(patch, 'audio').params.value).toHaveLength(1);
    });

    test('a control driven by a lane tap meters the resolved source', () => {
        const patch = execPatch(`
            $saw('c2').out({ label: 'bass' })
            $sine('c5').out({ label: 'lead', pan: $outLane('bass').pan })
        `);
        const lead = vuMeters(patch).find((m) => m.key === 'lead')!;
        expect(lead.panSource).toEqual({
            channel: 0,
            moduleId: '__vuPan_bass',
            portName: 'output',
        });
    });

    test('an undeclared label throws', () => {
        expect(() =>
            exec(`
                $sine('c4').out({ label: 'lead' })
                $signal($outLane('laed'), { id: 'probe' })
            `),
        ).toThrow(/no out is labeled "laed"/);
    });

    test('an empty label throws', () => {
        expect(() => exec(`$outLane('')`)).toThrow(/non-empty string/);
    });
});
