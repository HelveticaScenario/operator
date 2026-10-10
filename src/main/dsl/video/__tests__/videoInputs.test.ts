import { describe, expect, it } from 'vitest';
import schemas from '@modular/core/schemas.json';
import { buildLibSource } from '../../typescriptLibGen';
import { exec } from './videoExec';

describe('$v inputs from the audio graph', () => {
    describe('control-bound inputs', () => {
        it('binds a slider to a uniform slot carrying its current value', () => {
            const { video } = exec(`
                const freq = $slider('Freq', 4, 1, 10);
                $v.out($v.colorize($v.osc($v.ramp(), freq), 0, 0));
            `);
            expect(video!.uniforms).toEqual([
                {
                    kind: 'control',
                    slot: 0,
                    moduleId: '__slider_Freq',
                    value: 4,
                },
            ]);
            expect(video!.wgsl).toContain('u.slots[0][0]');
        });

        it('shares one slot between every use of a control', () => {
            const { video } = exec(`
                const amt = $slider('Amt', 0.5, 0, 1);
                const a = $v.osc($v.ramp(), 3, amt);
                $v.out($v.colorize(a, amt, $slider('Other', 1, 0, 2)));
            `);
            expect(
                video!.uniforms.map((u) =>
                    u.kind === 'control' ? u.moduleId : u.kind,
                ),
            ).toEqual(['__slider_Amt', '__slider_Other']);
        });

        it('binds a button as 0 until pressed', () => {
            const { video } = exec(`
                $v.out($v.colorize($btn('Flash'), 0, 0));
            `);
            expect(video!.uniforms).toEqual([
                {
                    kind: 'control',
                    slot: 0,
                    moduleId: '__button_Flash',
                    value: 0,
                },
            ]);
        });
    });

    describe('audio-signal inputs', () => {
        const taps = (source: string) =>
            exec(source).patch.modules.filter(
                (m) => m.moduleType === '_videoTap',
            );

        it('publishes an audio signal through a tap bound to a uniform slot', () => {
            const result = exec(`
                $v.out($v.colorize($sine('1hz').range(0, 1), 0, 0));
            `);
            expect(result.video!.uniforms).toEqual([
                { kind: 'tap', slot: 0, tap: 0, value: 0 },
            ]);
            expect(result.video!.wgsl).toContain('u.slots[0][0]');
            const [tap] = result.patch.modules.filter(
                (m) => m.moduleType === '_videoTap',
            );
            expect(tap.params).toMatchObject({ slot: 0 });
        });

        it('shares one tap between every use of a signal', () => {
            const found = taps(`
                const lfo = $sine('1hz').range(0, 1);
                $v.out($v.colorize(lfo, $v.osc($v.ramp(), lfo), 0));
            `);
            expect(found).toHaveLength(1);
        });

        it('gives different signals different taps', () => {
            const { video } = exec(`
                const a = $sine('1hz').range(0, 1);
                const b = $saw('2hz').range(0, 1);
                $v.out($v.colorize(a, b, 0));
            `);
            expect(
                video!.uniforms.map((u) => u.kind === 'tap' && u.tap),
            ).toEqual([0, 1]);
        });

        it('uses a control directly rather than through a tap', () => {
            expect(
                taps(`$v.out($v.colorize($slider('Level', 1, 0, 1), 0, 0));`),
            ).toHaveLength(0);
        });

        it('rejects a polyphonic signal', () => {
            expect(() =>
                exec(
                    `$v.out($v.colorize($c($sine('1hz'), $sine('2hz')), 0, 0));`,
                ),
            ).toThrow(/\$v\.colorize: r has 2 channels/);
        });

        it('rejects more signals than the engine has taps', () => {
            const reads = Array.from(
                { length: 65 },
                (_, i) => `$v.osc($v.ramp(), $sine(${i + 1}))`,
            ).join(', ');
            expect(() =>
                exec(`const f = [${reads}]; $v.out($v.colorize(f[0], 0, 0));`),
            ).toThrow(/at most 64 audio signals/);
        });

        it('keeps the internal tap module out of the user namespace', () => {
            expect(() => exec(`_videoTap($sine('1hz'), 0);`)).toThrow();
        });
    });

    it('does not declare the internal tap module in the DSL typings', () => {
        expect(buildLibSource(schemas as never, null)).not.toContain(
            '_videoTap',
        );
    });

    describe('patterns', () => {
        it('plays a pattern wherever a field is accepted, through a $cycle', () => {
            const { video, patch } = exec(`
                $v.out($v.hsv($p('0 1.25 2.5 3.75'), 5, 5));
            `);
            expect(video!.uniforms.map((u) => u.kind)).toEqual(['tap']);
            expect(
                Object.values(patch.modules).some(
                    (m) => m.moduleType === '$cycle',
                ),
            ).toBe(true);
        });

        it('accepts the sample, arrange and chained pattern forms too', () => {
            for (const pattern of [
                `$p.s('0 2 4', 'C(major)')`,
                `$p.arrange([2, $p('0 5')], [1, $p('2.5')])`,
                `$p('0 5').fast(2)`,
                `$p('0 5').slow(2)`,
            ]) {
                const { video } = exec(`$v.out($v.hsv(${pattern}));`);
                expect(video!.uniforms.map((u) => u.kind)).toEqual(['tap']);
            }
        });

        it('plays a pattern given to a chained method or a natural input', () => {
            const { video } = exec(`
                $v.ramp('h', { zoom: $p('1 2') }).$.hsv($p('0 5')).out();
            `);
            expect(video!.uniforms).toHaveLength(2);
        });

        it('plays a pattern through fromAudio as it would an audio signal', () => {
            const { video } = exec(`
                $v.out($v.hsv($v.fromAudio($p('0 5'))));
            `);
            expect(video!.histories).toHaveLength(1);
        });

        it('rejects a pattern that makes several voices', () => {
            expect(() => exec(`$v.out($v.hsv($p('0,5')));`)).toThrow(
                /has 2 channels/,
            );
        });
    });

    describe('fromAudio', () => {
        it('reads a window of an audio signal along the horizontal ramp', () => {
            const { video, patch } = exec(`
                $v.out($v.colorize($v.fromAudio($sine('110hz'), $v.ramp(), { samples: 256 }), 0, 0));
            `);
            expect(video!.histories).toEqual([
                { samples: 256, tap: 0, trigger: true },
            ]);
            expect(video!.wgsl).toContain('history_sample(0, ');
            expect(
                patch.modules.filter((m) => m.moduleType === '_videoTap'),
            ).toHaveLength(1);
        });

        it('defaults to the horizontal ramp and 512 samples', () => {
            const { video } = exec(
                `$v.out($v.colorize($v.fromAudio($saw('55hz')), 0, 0));`,
            );
            expect(video!.histories[0]).toMatchObject({
                samples: 512,
                trigger: true,
            });
            expect(video!.wgsl).toContain('let v0: f32 = uv.x;');
        });

        it('shares a row between identical reads and a tap with value reads', () => {
            const { video, patch } = exec(`
                const lfo = $sine('2hz');
                const a = $v.fromAudio(lfo);
                const b = $v.fromAudio(lfo, $v.ramp('v'));
                $v.out($v.colorize(a, b, lfo));
            `);
            expect(video!.histories).toHaveLength(1);
            expect(
                patch.modules.filter((m) => m.moduleType === '_videoTap'),
            ).toHaveLength(1);
            expect(video!.uniforms.map((u) => u.kind)).toEqual(['tap']);
        });

        it('gives a different window length its own row', () => {
            const { video } = exec(`
                const lfo = $sine('2hz');
                $v.out($v.colorize($v.fromAudio(lfo, 0.5, { samples: 64 }), $v.fromAudio(lfo, 0.5, { samples: 128 }), 0));
            `);
            expect(video!.histories.map((h) => h.samples)).toEqual([64, 128]);
        });

        it('rejects a window outside the ring', () => {
            expect(() =>
                exec(`$v.fromAudio($sine('1hz'), 0.5, { samples: 5000 });`),
            ).toThrow(
                /\$v\.fromAudio: samples must be an integer from 2 to 4096/,
            );
            expect(() =>
                exec(`$v.fromAudio($sine('1hz'), 0.5, { samples: 1.5 });`),
            ).toThrow(/samples must be an integer/);
        });

        it('rejects a video signal or a polyphonic signal', () => {
            expect(() => exec(`$v.fromAudio($v.ramp());`)).toThrow(
                /\$v\.fromAudio: signal must be a single-channel audio signal/,
            );
            expect(() =>
                exec(`$v.fromAudio($c($sine('1hz'), $sine('2hz')));`),
            ).toThrow(/single-channel audio signal/);
        });
    });

    describe('toCV', () => {
        it('returns a 0..1 audio signal fed by a region of the video', () => {
            const result = exec(`
                const level = $v.toCV($v.ramp(), { x: 0.25, y: 0.75, size: 0.1 });
                $sine(level.range(100, 200)).out();
                $v.out($v.hsv(0.5));
            `);
            expect(result.video!.cvSamples).toEqual([
                { id: '__videoCV_0', index: 0, size: 0.1, x: 0.25, y: 0.75 },
            ]);
            const cv = result.patch.modules.find((m) => m.id === '__videoCV_0');
            expect(cv?.moduleType).toBe('$signal');
        });

        it('defaults to the whole frame', () => {
            const { video } = exec(`
                $v.toCV($v.ramp());
                $v.out($v.hsv(0.5));
            `);
            expect(video!.cvSamples[0]).toMatchObject({
                size: 0.5,
                x: 0.5,
                y: 0.5,
            });
        });

        it('numbers its preview slots among the editor previews', () => {
            const { video, videoPreviews } = exec(`
                $v.preview($v.ramp());
                $v.toCV($v.ramp('v'));
                $v.out($v.preview($v.hsv(0.5), { view: 'vectorscope' }));
            `);
            expect(videoPreviews.map((p) => p.index)).toEqual([0, 2]);
            expect(video!.cvSamples.map((c) => c.index)).toEqual([1]);
            expect(video!.previewCount).toBe(3);
        });

        it('closes the loop: video drives audio drives video', () => {
            const { video } = exec(`
                const level = $v.toCV($v.ramp(), { x: 0.9, size: 0.02 });
                $v.out($v.colorize(level, level, level));
            `);
            expect(video!.uniforms.map((u) => u.kind)).toEqual(['tap']);
            expect(video!.cvSamples).toHaveLength(1);
        });

        it('rejects a region that is empty or not finite', () => {
            expect(() => exec(`$v.toCV($v.ramp(), { size: 0 });`)).toThrow(
                /\$v\.toCV: size must be greater than 0/,
            );
            expect(() => exec(`$v.toCV($v.ramp(), { x: 'left' });`)).toThrow(
                /\$v\.toCV: x must be a finite number/,
            );
        });

        it('rejects something that is not a video signal', () => {
            expect(() => exec(`$v.toCV(0.5);`)).toThrow(
                /\$v\.toCV: signal must be a video field or color/,
            );
        });
    });
});
