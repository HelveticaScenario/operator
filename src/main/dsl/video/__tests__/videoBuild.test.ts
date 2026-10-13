import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';

const modulesOf = (source: string, type: string) =>
    exec(source).patch.modules.filter((m) => m.moduleType === type);

describe('$v graph building', () => {
    describe('pruning', () => {
        it('opens no media that the picture does not read', () => {
            const { video } = exec(`
                const cam = $v.camera();
                $v.image('a.png');
                $v.out($v.osc(10));
            `);
            expect(video!.sources).toEqual([]);
            expect(video!.wgsl).not.toContain('src_');
        });

        it('numbers the media that are read from 0', () => {
            const { video } = exec(`
                $v.image('a.png');
                $v.out($v.image('b.png'));
            `);
            expect(video!.sources).toEqual([{ kind: 'image', path: 'b.png' }]);
            expect(video!.wgsl).toContain('var src_0: texture_2d<f32>;');
            expect(video!.wgsl).not.toContain('src_1');
        });

        it('publishes no tap and keeps no row for an unread fromAudio', () => {
            const source = `
                $v.fromAudio($sine('1hz'));
                $v.out($v.osc(10));
            `;
            expect(exec(source).video!.histories).toEqual([]);
            expect(modulesOf(source, '_videoTap')).toHaveLength(0);
        });

        it('numbers the rows that are read from 0', () => {
            const { video } = exec(`
                const lfo = $sine('1hz');
                $v.fromAudio(lfo, 0.5, { samples: 64 });
                $v.out($v.fromAudio(lfo, 0.5, { samples: 128 }));
            `);
            expect(video!.histories).toEqual([
                { samples: 128, tap: 0, trigger: true },
            ]);
            expect(video!.wgsl).toContain('history_sample(0, ');
        });

        it('publishes only the taps of signals the picture reads, numbered from 0', () => {
            const source = `
                $v.osc($sine('1hz'));
                $v.out($v.osc($saw('2hz')));
            `;
            const { video, patch } = exec(source);
            expect(video!.uniforms).toEqual([
                { kind: 'tap', slot: 0, tap: 0, value: 0 },
            ]);
            expect(video!.wgsl).toContain('u.slots[0][0]');
            const taps = patch.modules.filter(
                (m) => m.moduleType === '_videoTap',
            );
            expect(taps).toHaveLength(1);
            expect(taps[0].params.slot).toBe(0);
            const saw = patch.modules.find((m) => m.moduleType === '$saw')!;
            expect(JSON.stringify(taps[0].params.input)).toContain(saw.id);
        });

        it('keeps only the controls the picture reads', () => {
            const { video } = exec(`
                $v.osc($slider('A', 1, 0, 2));
                $v.out($v.osc($slider('B', 1, 0, 2)));
            `);
            expect(video!.uniforms).toEqual([
                { kind: 'control', slot: 0, moduleId: '__slider_B', value: 1 },
            ]);
        });

        it('publishes nothing for a patch without output or previews', () => {
            expect(
                modulesOf(`$v.osc($sine('1hz'));`, '_videoTap'),
            ).toHaveLength(0);
        });

        it("plays a video's audio when the picture does not show the video", () => {
            const source = `
                const clip = $v.video('a.mp4');
                clip.audio.out();
                $v.out($v.osc(10));
            `;
            expect(exec(source).video!.sources).toEqual([]);
            expect(modulesOf(source, '_mediaAudio')).toHaveLength(1);
        });

        it('keeps a shown video and its audio', () => {
            const source = `
                const clip = $v.video('a.mp4');
                clip.audio.out();
                $v.out(clip);
            `;
            expect(exec(source).video!.sources).toEqual([
                { kind: 'video', path: 'a.mp4' },
            ]);
            expect(modulesOf(source, '_mediaAudio')).toHaveLength(1);
        });
    });

    describe('patterns', () => {
        it('plays one pattern used twice through one $cycle and one tap', () => {
            const source = `
                const p = $p('0 1');
                $v.out($v.hsv(p, p));
            `;
            expect(modulesOf(source, '$cycle')).toHaveLength(1);
            expect(modulesOf(source, '_videoTap')).toHaveLength(1);
            expect(exec(source).video!.uniforms).toHaveLength(1);
        });

        it('shares the $cycle between a field and fromAudio', () => {
            const source = `
                const p = $p('0 1');
                $v.out($v.hsv(p, $v.fromAudio(p)));
            `;
            expect(modulesOf(source, '$cycle')).toHaveLength(1);
            expect(modulesOf(source, '_videoTap')).toHaveLength(1);
        });
    });

    describe('textures', () => {
        const images = (count: number) =>
            `Array.from({ length: ${count} }, (_, i) => $v.image('i' + i + '.png')).reduce((a, b) => $v.add(a, b))`;

        it('accepts 16 textures', () => {
            expect(() =>
                exec(
                    `$v.out($v.add($v.frameDelay($v.osc(), 7), ${images(9)}));`,
                ),
            ).not.toThrow();
        });

        it('rejects more than 16 textures and says what used them', () => {
            expect(() =>
                exec(`
                    const sound = $v.fromAudio($sine('1hz'));
                    $v.out($v.add($v.add($v.frameDelay(sound, 7), sound), ${images(9)}));
                `),
            ).toThrow(
                /\$v: a patch can use at most 16 textures.*7 feedback buffers, 1 audio history and 9 sources/,
            );
        });

        it('counts only the textures the picture reads', () => {
            expect(() =>
                exec(`
                    ${images(4)};
                    $v.out($v.add($v.frameDelay($v.osc(), 7), ${images(9)}));
                `),
            ).not.toThrow();
        });
    });

    describe('constants', () => {
        it('rejects a constant that is not a number a shader can hold, naming the function', () => {
            for (const bad of [
                'NaN',
                'Infinity',
                '-Infinity',
                '1e39',
                '-4e38',
            ]) {
                expect(() => exec(`$v.out($v.osc(${bad}));`)).toThrow(
                    /\$v\.osc: freq must be a finite number within ±3\.4e38/,
                );
            }
        });

        it('accepts the largest f32', () => {
            expect(() => exec(`$v.out($v.osc(3.4e38));`)).not.toThrow();
        });
    });

    describe('buffers', () => {
        it('rejects a buffer that is read but never written, in DSL terms', () => {
            expect(() =>
                exec(`
                    const b = $v.buffer();
                    $v.out(b.read());
                `),
            ).toThrow(/\$v\.buffer: a buffer is read but never written/);
        });

        it('accepts an unwritten buffer the picture does not read', () => {
            expect(() =>
                exec(`
                    const b = $v.buffer();
                    b.read();
                    $v.out($v.osc());
                `),
            ).not.toThrow();
        });
    });

    describe('cost', () => {
        it('accepts two nested blurs', () => {
            expect(() =>
                exec(`$v.out($v.blur($v.blur($v.osc())));`),
            ).not.toThrow();
        });

        it('rejects three nested blurs, which re-evaluate the input thousands of times a pixel', () => {
            expect(() =>
                exec(`$v.out($v.blur($v.blur($v.blur($v.osc()))));`),
            ).toThrow(/\$v: the picture costs \d+ evaluations per pixel/);
        });

        it('multiplies the samples of edges and blurs', () => {
            expect(() =>
                exec(`$v.out($v.edges($v.blur($v.blur($v.osc()))));`),
            ).toThrow(/evaluations per pixel/);
        });

        it('pays a shared input once per pixel outside warps', () => {
            expect(() =>
                exec(`
                    let x = $v.blur($v.blur($v.osc()));
                    for (let i = 0; i < 8; i++) x = $v.add(x, x);
                    $v.out(x);
                `),
            ).not.toThrow();
        });
    });
});
