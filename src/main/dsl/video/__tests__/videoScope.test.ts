import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';

const taps = (patch: ReturnType<typeof exec>['patch']) =>
    patch.modules.filter((m) => m.moduleType === '_videoTap');

describe('$v.scopeXY', () => {
    it('draws one trace from an x and a y signal with the default settings', () => {
        const { patch, video } = exec(`
            $v.scopeXY($sine('440hz'), $sine('330hz')).out()
        `);
        expect(video!.scopes).toEqual([
            {
                intensity: { kind: 'const', value: 0.6 },
                lineWidth: { kind: 'const', value: 1 },
                persistence: { kind: 'const', value: 0.6 },
                samples: 2048,
                traces: [{ x: 0, y: 1 }],
                upsample: false,
                xRange: [-5, 5],
                yRange: [-5, 5],
            },
        ]);
        expect(video!.histories).toEqual([
            { samples: 2048, tap: 0, trigger: false },
            { samples: 2048, tap: 1, trigger: false },
        ]);
        expect(taps(patch)).toHaveLength(2);
        expect(video!.wgsl).toContain('var scope_0: texture_2d<f32>');
        expect(video!.wgsl).toContain('video_scope(scope_0, uv)');
    });

    it('pairs channels off, cycling the shorter side, one tap per channel', () => {
        const { patch, video } = exec(`
            $v.scopeXY($sine(['c3', 'e3', 'g3']), $sine('c2')).out()
        `);
        const [scope] = video!.scopes;
        expect(scope.traces).toHaveLength(3);
        expect(new Set(scope.traces.map((t) => t.y)).size).toBe(1);
        expect(new Set(scope.traces.map((t) => t.x)).size).toBe(3);
        expect(taps(patch)).toHaveLength(4);
    });

    it('takes an axis mixing collections, numbers, notes and patterns', () => {
        const { video } = exec(`
            $scopeXY(
                [$sine('c3').range(-1, 1), 2, 'e3', $p('c3 e3')],
                $saw(['c2', 'g2']),
            )
        `);
        expect(video!.scopes[0].traces).toHaveLength(4);
    });

    it('lifts numbers and note strings into constant signals', () => {
        const { video } = exec(`$v.scopeXY($sine('c4'), 2.5).out()`);
        expect(video!.scopes[0].traces).toHaveLength(1);
    });

    it('passes the config through', () => {
        const { video } = exec(`
            $v.scopeXY($sine('c4'), $sine('g4'), {
                xRange: [-1, 1],
                yRange: [0, 10],
                samples: 512,
                intensity: 0.9,
                persistence: 0.2,
                lineWidth: 2.5,
                upsample: true,
            }).out()
        `);
        expect(video!.scopes[0]).toMatchObject({
            intensity: { kind: 'const', value: 0.9 },
            lineWidth: { kind: 'const', value: 2.5 },
            persistence: { kind: 'const', value: 0.2 },
            samples: 512,
            upsample: true,
            xRange: [-1, 1],
            yRange: [0, 10],
        });
    });

    it('rejects a reversed or non-finite range', () => {
        expect(() =>
            exec(
                `$v.scopeXY($sine('c4'), $sine('c4'), { xRange: [5, -5] }).out()`,
            ),
        ).toThrow(/xRange/);
        expect(() =>
            exec(
                `$v.scopeXY($sine('c4'), $sine('c4'), { yRange: [0, Infinity] }).out()`,
            ),
        ).toThrow(/yRange/);
    });

    it('rejects settings out of range', () => {
        expect(() =>
            exec(
                `$v.scopeXY($sine('c4'), $sine('c4'), { persistence: 2 }).out()`,
            ),
        ).toThrow(/persistence/);
        expect(() =>
            exec(`$v.scopeXY($sine('c4'), $sine('c4'), { samples: 1 }).out()`),
        ).toThrow(/samples/);
    });

    it('rejects an axis with no channels', () => {
        expect(() => exec(`$v.scopeXY([], $sine('c4')).out()`)).toThrow(
            /\$v\.scopeXY: x/,
        );
    });

    it('drops a scope the picture does not use, with its signals', () => {
        const { patch, video } = exec(`
            $v.scopeXY($sine('c4'), $sine('g4'))
            $v.osc(4).out()
        `);
        expect(video!.scopes).toEqual([]);
        expect(video!.histories).toEqual([]);
        expect(taps(patch)).toHaveLength(0);
    });

    it('can be warped and composited like any signal', () => {
        const { video } = exec(`
            $v.out($v.mult($v.scopeXY($sine('c4'), $sine('g4')).$.kaleid(4), $v.hsv(0.9, 1, 1)))
        `);
        expect(video!.scopes).toHaveLength(1);
        expect(video!.wgsl).toContain('video_scope(scope_0, uv)');
    });

    it('numbers several scopes and their history rows densely', () => {
        const { video } = exec(`
            const a = $v.scopeXY($sine('c4'), $sine('g4'))
            const b = $v.scopeXY($sine('c4'), $sine('e4'), { samples: 1024 })
            $v.add(a, b).out()
        `);
        expect(video!.scopes.map((s) => s.traces)).toEqual([
            [{ x: 0, y: 1 }],
            [{ x: 2, y: 3 }],
        ]);
        expect(video!.wgsl).toContain('var scope_1: texture_2d<f32>');
    });
});

describe('$scopeXY', () => {
    it('shows $v.scopeXY as the picture', () => {
        const { video } = exec(`$scopeXY($sine('440hz'), $sine('330hz'))`);
        expect(video!.hasOutput).toBe(true);
        expect(video!.scopes).toHaveLength(1);
    });

    it('returns the scope so it can be reused', () => {
        const { video } = exec(`
            const s = $scopeXY($sine('440hz'), $sine('330hz'))
            $v.out($v.mult(s, $v.hsv(0.5, 1, 1)))
        `);
        expect(video!.scopes).toHaveLength(1);
    });
});

describe('$v.scopeXY settings driven by signals', () => {
    it('binds a slider to a uniform slot the scope reads', () => {
        const { video } = exec(`
            $v.scopeXY($sine('c4'), $sine('g4'), {
                intensity: $slider('Beam', 0.8, 0, 1),
            }).out()
        `);
        const [scope] = video!.scopes;
        expect(scope.intensity).toEqual({ kind: 'uniform', slot: 0 });
        expect(video!.uniforms).toEqual([
            { kind: 'control', moduleId: '__slider_Beam', slot: 0, value: 0.8 },
        ]);
        expect(scope.persistence).toEqual({ kind: 'const', value: 0.6 });
    });

    it('binds an audio signal through a tap', () => {
        const { video } = exec(`
            $v.scopeXY($sine('c4'), $sine('g4'), {
                lineWidth: $sine('1hz').range(0.5, 3),
            }).out()
        `);
        const [scope] = video!.scopes;
        expect(scope.lineWidth.kind).toBe('uniform');
        const slot = (scope.lineWidth as { slot: number }).slot;
        expect(video!.uniforms[slot].kind).toBe('tap');
    });

    it('plays a pattern', () => {
        const { video } = exec(`
            $v.scopeXY($sine('c4'), $sine('g4'), {
                persistence: $p('0.2 0.9'),
            }).out()
        `);
        expect(video!.scopes[0].persistence.kind).toBe('uniform');
    });

    it('keeps the slots of settings when renumbering', () => {
        const { video } = exec(`
            const unused = $v.solid($slider('Unused', 1, 0, 1), 0, 0)
            $v.scopeXY($sine('c4'), $sine('g4'), {
                intensity: $slider('Beam', 0.8, 0, 1),
            }).out()
        `);
        expect(video!.scopes[0].intensity).toEqual({
            kind: 'uniform',
            slot: 0,
        });
        expect(video!.uniforms).toHaveLength(1);
    });

    it('rejects a video signal', () => {
        expect(() =>
            exec(
                `$v.scopeXY($sine('c4'), $sine('g4'), { intensity: $v.osc(2) }).out()`,
            ),
        ).toThrow(
            /intensity takes a number, a slider, a pattern or an audio signal/,
        );
    });
});
