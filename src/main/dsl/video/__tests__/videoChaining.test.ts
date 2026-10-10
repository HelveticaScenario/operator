import { describe, expect, it } from 'vitest';
import { VIDEO_CHAIN } from '../../../../shared/dsl/videoDocs';
import { VIDEO_CHAIN_METHODS, VIDEO_DIRECT_METHODS } from '../VideoOutput';
import { exec } from './videoExec';

describe('$v chaining', () => {
    it('documents exactly the methods a video signal chains with', () => {
        expect(
            [...VIDEO_CHAIN_METHODS, ...VIDEO_DIRECT_METHODS].sort(),
        ).toEqual(VIDEO_CHAIN.map((m) => m.name).sort());
        expect(
            VIDEO_CHAIN.filter((m) => m.direct)
                .map((m) => m.name)
                .sort(),
        ).toEqual([...VIDEO_DIRECT_METHODS].sort());
    });

    describe('chaining', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('compiles a .$ chain to the same shader as nested calls', () => {
            const chained = wgslOf(`
                $v.osc($v.ramp(), 8).$.kaleid(6).$.hsv(0.9).out();
            `);
            const nested = wgslOf(`
                $v.out($v.hsv($v.kaleid($v.osc($v.ramp(), 8), 6), 0.9));
            `);
            expect(chained).toBe(nested);
        });

        it('treats rotate, scale and scroll as shorthand for warp', () => {
            const shorthand = wgslOf(`
                $v.ramp().$.rotate(0.1).$.scale(2).$.scroll(0.2, 0.3).$.hsv().out();
            `);
            const long = wgslOf(`
                $v.out($v.hsv($v.warp($v.warp($v.warp($v.ramp(), { rotate: 0.1 }), { zoom: 2 }), { shiftX: 0.2, shiftY: 0.3 })));
            `);
            expect(shorthand).toBe(long);
        });

        it('chains colors and fields with the right variants', () => {
            const wgsl = wgslOf(`
                $v.noise($v.ramp(), $v.ramp('v')).$.hsv().$.mult($v.ramp('v')).$.invert().out();
            `);
            expect(wgsl).toMatch(/let v\d: vec3f = v\d \* v\d;/);
            expect(wgsl).toMatch(/vec3f\(1\.0\) - /);
        });

        it('keeps the chain methods off the signal itself', () => {
            const { patch } = exec(`
                const signal = $v.ramp();
                $sine(signal.rotate === undefined ? 100 : 200).out();
            `);
            const sine = patch.modules.find((m) => m.moduleType === '$sine');
            expect(JSON.stringify(sine?.params)).toContain('100');
        });

        it('returns nothing for a name that is not a chain function', () => {
            const { patch } = exec(`
                $sine($v.ramp().$.nope === undefined ? 100 : 200).out();
            `);
            const sine = patch.modules.find((m) => m.moduleType === '$sine');
            expect(JSON.stringify(sine?.params)).toContain('100');
        });

        it('previews and measures partway through a chain', () => {
            const { video, videoPreviews } = exec(`
                const wave = $v.osc($v.ramp(), 4).preview({ view: 'waveform' });
                wave.toCV({ size: 0.1 });
                wave.$.hsv().out();
            `);
            expect(videoPreviews.map((p) => p.view)).toEqual(['waveform']);
            expect(video!.cvSamples).toHaveLength(1);
        });

        it('reports errors with the $v function that failed', () => {
            expect(() => exec(`$v.ramp().$.kaleid('six');`)).toThrow(
                /\$v\.kaleid: sides must be a number or a video field/,
            );
            expect(() => exec(`$v.ramp().$.hueShift('slow');`)).toThrow(
                /\$v\.hueShift: amount must be a number or a video field/,
            );
        });

        describe('.$m', () => {
            it('crossfades the signal against the result with a leading mix', () => {
                const mixed = wgslOf(`
                    $v.hsv($v.ramp()).$m.hueShift(0.3, 0.25).out();
                `);
                const long = wgslOf(`
                    const c = $v.hsv($v.ramp());
                    $v.out($v.mix(c, $v.hueShift(c, 0.25), 0.3));
                `);
                expect(mixed).toBe(long);
            });

            it('takes only the mix for a function with no other arguments', () => {
                const mixed = wgslOf(`$v.hsv($v.ramp()).$m.invert(2.5).out();`);
                expect(mixed).toMatch(/mix\(v\d, v\d, clamp\(0\.5/);
            });

            it('mixes a field result with a field signal', () => {
                const wgsl = wgslOf(`
                    $v.osc($v.ramp(), 4).$m.fold(0.5, 2).$.hsv().out();
                `);
                expect(wgsl).toMatch(/let v\d: f32 = mix\(v\d, /);
            });

            it('accepts a signal as the mix', () => {
                const { video } = exec(`
                    $v.hsv($v.ramp()).$m.invert($slider('Amount', 0.5, 0, 1)).out();
                `);
                expect(video!.uniforms).toHaveLength(1);
            });
        });

        describe('.pipe', () => {
            it('calls a function with the signal', () => {
                const piped = wgslOf(`
                    $v.hsv($v.ramp()).pipe((c) => c.$.kaleid(5).$.hueShift(0.3)).out();
                `);
                const long = wgslOf(`
                    $v.out($v.hueShift($v.kaleid($v.hsv($v.ramp()), 5), 0.3));
                `);
                expect(piped).toBe(long);
            });

            it('calls a function once per array element and returns the results', () => {
                const { video } = exec(`
                    const layers = $v.hsv($v.ramp()).pipe(
                        (c, sides) => c.$.kaleid(sides),
                        [3, 5, 7],
                    );
                    $v.out(layers.reduce((sum, layer) => sum.$.add(layer.$.mult(0.3))));
                `);
                expect(
                    video!.wgsl.match(/video_kaleid\(uv, [357]\.0, 0\.0\)/g),
                ).toHaveLength(3);
            });

            it('rejects something that is not a function or an array', () => {
                expect(() => exec(`$v.ramp().pipe(3);`)).toThrow(
                    /pipe: expects a function/,
                );
                expect(() => exec(`$v.ramp().pipe((s) => s, 3);`)).toThrow(
                    /pipe: the second argument must be an array/,
                );
            });
        });

        describe('.pipeMix', () => {
            it('crossfades the signal against the function result, half way by default', () => {
                const mixed = wgslOf(`
                    $v.hsv($v.ramp()).pipeMix((c) => c.$.invert()).out();
                `);
                const long = wgslOf(`
                    const c = $v.hsv($v.ramp());
                    $v.out($v.mix(c, $v.invert(c), 2.5));
                `);
                expect(mixed).toBe(long);
            });

            it('takes the mix as a signal', () => {
                const { video } = exec(`
                    $v.hsv($v.ramp()).pipeMix((c) => c.$.invert(), $v.osc($v.time, 0.2)).out();
                `);
                expect(video!.wgsl).toContain('mix(');
            });

            it('rejects something that is not a function', () => {
                expect(() => exec(`$v.ramp().pipeMix(3);`)).toThrow(
                    /pipeMix: expects a function/,
                );
            });
        });
    });

    describe('osc and out in a chain', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('chains osc to the same shader as the nested call', () => {
            expect(
                wgslOf(`$v.ramp().$.osc(8, 0.5, { shape: 'saw' }).out();`),
            ).toBe(
                wgslOf(`$v.out($v.osc($v.ramp(), 8, 0.5, { shape: 'saw' }));`),
            );
        });

        it('mixes an osc into the signal with $m', () => {
            expect(() => exec(`$v.ramp().$m.osc(2.5, 8).out();`)).not.toThrow();
        });

        it('returns the signal from out, so the chain can go on', () => {
            const once = wgslOf(`$v.ramp().$.hsv().out();`);
            const twice = wgslOf(`
                const c = $v.ramp().$.hsv().out();
                c.$.invert();
            `);
            expect(twice).toBe(once);
        });

        it('returns the input from $v.out and keeps the last call as the picture', () => {
            const { video } = exec(`
                const a = $v.out($v.colorize(5, 0, 0));
                $v.out(a.$.invert());
            `);
            expect(video!.wgsl).toContain('1.0 - ');
        });
    });

    describe('channel properties', () => {
        it('gives a color its r, g and b as fields, and a field none', () => {
            const { video } = exec(`
                const c = $v.hsv($v.ramp());
                const f = $v.ramp();
                if (f.r !== undefined || f.g !== undefined || f.b !== undefined) {
                    throw new Error('a field has channels');
                }
                $v.out($v.colorize(c.b, c.r, c.g));
            `);
            expect(video).not.toBeNull();
        });
    });

    describe('tint', () => {
        it('colors a mask with a fixed hue, using the field as brightness', () => {
            const tinted = exec(`$v.ramp().$.tint(0.3, 0.8).out();`).video!
                .wgsl;
            const long = exec(`$v.out($v.hsv(0.3, 0.8, $v.ramp()));`).video!
                .wgsl;
            expect(tinted).toBe(long);
        });

        it('differs from hsv, which uses the field as the hue', () => {
            const tinted = exec(`$v.ramp().$.tint(0.3).out();`).video!.wgsl;
            const hued = exec(`$v.ramp().$.hsv(0.3).out();`).video!.wgsl;
            expect(tinted).not.toBe(hued);
        });

        it('defaults to a red of full saturation', () => {
            const tinted = exec(`$v.ramp().$.tint().out();`).video!.wgsl;
            expect(tinted).toContain('hsv_to_rgb(0.0, 1.0, v0)');
        });

        it('is not available on a color', () => {
            expect(() => exec(`$v.hsv(0.2).$.tint(0.1);`)).toThrow();
        });
    });

    describe('channels and swizzles', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('exposes the channels of a color as r, g and b', () => {
            const props = wgslOf(`
                const c = $v.hsv($v.ramp());
                $v.out($v.colorize(c.b, c.r, c.g));
            `);
            const calls = wgslOf(`
                const c = $v.hsv($v.ramp());
                $v.out($v.colorize($v.channel(c, 'b'), $v.channel(c, 'r'), $v.channel(c, 'g')));
            `);
            expect(props).toBe(calls);
        });

        it('still takes a field as its own channel through $v.channel', () => {
            expect(
                wgslOf(
                    `const f = $v.ramp(); $v.out($v.hsv($v.channel(f, 'g')));`,
                ),
            ).toBe(wgslOf(`const f = $v.ramp(); $v.out($v.hsv(f));`));
        });

        it('remaps the channels of a color', () => {
            const wgsl = wgslOf(`$v.hsv($v.ramp()).$.swiz('gbr').out();`);
            expect(wgsl).toMatch(/\.gbr;/);
            expect(
                wgslOf(`$v.out($v.swiz($v.hsv($v.ramp()), 'rrr'));`),
            ).toMatch(/\.rrr;/);
        });

        it('swizzles a field as a gray color', () => {
            expect(wgslOf(`$v.out($v.swiz($v.ramp(), 'bgr'));`)).toMatch(
                /\.bgr;/,
            );
        });

        it('rejects a pattern that is not three channels', () => {
            for (const pattern of ["'rg'", "'rgba'", "'rgx'", '3']) {
                expect(() => exec(`$v.swiz($v.hsv(0), ${pattern});`)).toThrow(
                    /\$v\.swiz: pattern must be three of r, g and b/,
                );
            }
        });
    });
});
