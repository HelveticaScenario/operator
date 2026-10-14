import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';
import { callWgsl } from './wgslEval';

describe('$v modules', () => {
    describe('math and keying modules', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('combines whole signals through one helper per operation', () => {
            const wgsl = wgslOf(`
                const c = $v.hsv($v.ramp());
                $v.out($v.add($v.mult(c, $v.ramp('vertical')), $v.mult($v.ramp(), 0.5)));
            `);
            expect(wgsl.match(/fn video_mult\(/g)).toHaveLength(1);
            expect(wgsl).toMatch(/let v\d: vec4f = video_mult\(v\d, v\d\);/);
            expect(wgsl).toMatch(
                /video_mult\(v\d, vec4f\(vec3f\(0\.5\), 1\.0\)\)/,
            );
        });

        it('blends with an unclipped amount, half way by default', () => {
            expect(
                wgslOf(`$v.out($v.blend($v.ramp(), $v.solid(1, 0, 0), 1.5));`),
            ).toMatch(/let v\d: vec4f = mix\(v\d, v\d, 1\.5\);/);
            expect(
                wgslOf(`$v.ramp().$.blend($v.solid(1, 0, 0)).out();`),
            ).toMatch(/let v\d: vec4f = mix\(v\d, v\d, 0\.5\);/);
        });

        it('gives a number to a signal operand as an opaque gray', () => {
            const wgsl = wgslOf(`
                $v.out($v.add($v.solid(1, 0, 0), 0.25));
            `);
            expect(wgsl).toContain('vec4f(vec3f(0.25), 1.0)');
        });

        it('declares each module helper once however often the module is used', () => {
            const wgsl = wgslOf(`
                const a = $v.shape(0.2);
                const b = $v.shape(0.1, 0, { shape: 'box' });
                $v.out($v.key($v.hsv(a), $v.procAmp($v.hsv(b)), $v.comparator(a)));
            `);
            expect(wgsl.match(/fn shape_edge/g)).toHaveLength(1);
            expect(wgsl.match(/fn hsv_to_rgb/g)).toHaveLength(1);
            expect(wgsl).toContain('fn procamp');
        });

        it('reads the brightness of a signal given where a number is wanted', () => {
            const wgsl = wgslOf(
                `$v.out($v.mix($v.ramp(), $v.ramp('vertical'), $v.hsv(0.3)));`,
            );
            expect(wgsl).toMatch(
                /mix\(v\d, v\d, clamp\(video_luma\(v\d\), 0\.0, 1\.0\)\)/,
            );
            expect(() => exec(`$v.comparator('x');`)).toThrow(
                /\$v\.comparator: input must be a number, a video signal or an audio signal/,
            );
        });

        it('rejects an invalid shape option', () => {
            expect(() =>
                exec(
                    `$v.out($v.hsv($v.shape(0.2, 0, { shape: 'star', mapX: 0.5, mapY: 0.5 })));`,
                ),
            ).toThrow(/param "shape" must be one of circle, box, diamond/);
        });
    });

    describe('coordinate ramps and waveshaping', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('compiles an untransformed ramp to the bare frame coordinate', () => {
            const wgsl = wgslOf(`$v.out($v.solid($v.ramp(), 0, 0));`);
            expect(wgsl).toContain('let v0: vec4f = vec4f(vec3f(uv.x), 1.0);');
        });

        it('routes a transformed ramp through video_transform', () => {
            const wgsl = wgslOf(
                `$v.out($v.solid($v.ramp('vertical', { rotate: 0.1, zoom: 2 }), 0, 0));`,
            );
            expect(wgsl).toContain(
                'video_transform(uv, 2.0, 0.1, vec2f(0.0, 0.0)).y',
            );
        });

        it('offers radial and angular ramps', () => {
            const wgsl = wgslOf(
                `$v.out($v.solid($v.ramp('radial'), $v.ramp('angular'), 0));`,
            );
            expect(wgsl).toContain('ramp_radius(uv)');
            expect(wgsl).toContain('ramp_angle(uv)');
        });

        it('declares the shared transform once when ramps and feedback both use it', () => {
            const wgsl = wgslOf(`
                const r = $v.ramp('horizontal', { rotate: 0.1 });
                $v.out($v.feedback((prev) => $v.mix($v.hsv(r), prev, 0.9), { rotate: 0.01 }));
            `);
            expect(wgsl.match(/fn video_transform/g)).toHaveLength(1);
        });

        it('lets a slider drive a ramp transform', () => {
            const { video } = exec(`
                const spin = $slider('Spin', 0, -0.5, 0.5);
                $v.out($v.hsv($v.ramp('angular', { rotate: spin })));
            `);
            expect(video!.uniforms).toHaveLength(1);
        });

        it('compiles min, max, wrap and fold', () => {
            const wgsl = wgslOf(`
                const x = $v.ramp();
                const y = $v.ramp('vertical');
                $v.out($v.solid(
                    $v.max($v.wrap(x, 3), $v.fold(y, 4)),
                    $v.min(x, y),
                    0,
                ));
            `);
            expect(wgsl).toContain('return vec4f(fract(x * gain), c.a);');
            expect(wgsl).toMatch(/video_max\(v\d, v\d\)/);
            expect(wgsl).toMatch(/video_min\(v\d, v\d\)/);
        });

        it('shapes each channel of a color and keeps its alpha', () => {
            const wgsl = wgslOf(
                `$v.hsv($v.ramp()).$.posterize(3).$.comparator(0.4).out();`,
            );
            expect(wgsl).toMatch(/video_posterize\(v\d, 3\.0, 0\.6\)/);
            expect(wgsl).toMatch(/video_comparator\(v\d, 0\.4, 0\.0\)/);
            expect(wgsl).toContain('return vec4f(clamp((x - vec3f(threshold))');
        });

        it('posterizes to finite levels at bins or gamma 0', () => {
            const wgsl = wgslOf(`$v.ramp().$.posterize().out();`);
            for (const [bins, gamma] of [
                [0, 0.6],
                [3, 0],
                [0, 0],
            ]) {
                for (const x of [0, 0.5, 1]) {
                    const out = callWgsl(
                        wgsl,
                        'video_posterize',
                        [x, x, x, 1],
                        bins,
                        gamma,
                    ) as number[];
                    expect(out.every(Number.isFinite)).toBe(true);
                }
            }
        });

        it("posterizes along a gamma curve, as Hydra's posterize does", () => {
            const wgsl = wgslOf(`$v.ramp().$.posterize().out();`);
            const level = (x: number, bins: number, gamma: number) =>
                (
                    callWgsl(
                        wgsl,
                        'video_posterize',
                        [x, x, x, 0.7],
                        bins,
                        gamma,
                    ) as number[]
                )[0];
            const hydra = (x: number, bins: number, gamma: number) =>
                Math.pow(
                    Math.floor(Math.pow(x, gamma) * bins) / bins,
                    1 / gamma,
                );
            for (const x of [0, 0.1, 0.3, 0.5, 0.75, 0.99, 1]) {
                expect(level(x, 3, 0.6)).toBeCloseTo(hydra(x, 3, 0.6), 6);
            }
            expect([0, 0.2, 0.4, 0.6, 0.8].map((x) => level(x, 3, 1))).toEqual([
                0,
                0,
                1 / 3,
                1 / 3,
                2 / 3,
            ]);
            expect(level(-0.5, 3, 0.6)).toBe(0);
            expect(
                (
                    callWgsl(
                        wgsl,
                        'video_posterize',
                        [0.5, 0.5, 0.5, 0.7],
                        3,
                        0.6,
                    ) as number[]
                )[3],
            ).toBe(0.7);
        });

        it('folds to match the input times the gain below 1 / gain', () => {
            const wgsl = wgslOf(`$v.ramp().$.fold(4).out();`);
            const fold = (x: number) =>
                (callWgsl(wgsl, 'video_fold', [x, x, x, 1], 4) as number[])[0];
            expect(fold(0.1)).toBeCloseTo(0.4);
            expect(fold(0.3)).toBeCloseTo(0.8);
            expect(fold(0.375)).toBeCloseTo(0.5);
        });

        it('swaps the sides of comparator with invert', () => {
            const above = wgslOf(`$v.ramp().$.comparator(0.3).out();`);
            const below = wgslOf(
                `$v.ramp().$.comparator(0.3, 0, { invert: true }).out();`,
            );
            expect(above).toContain('clamp((x - vec3f(threshold))');
            expect(below).toContain('clamp((vec3f(threshold) - x)');
            expect(below).toMatch(/video_comparator_below\(v\d, 0\.3, 0\.0\)/);
        });

        it('rejects an unknown ramp axis', () => {
            expect(() => exec(`$v.out($v.solid($v.ramp('q'), 0, 0));`)).toThrow(
                /param "axis" must be one of horizontal, vertical, radial, angular/,
            );
        });
    });

    describe('noise', () => {
        it('compiles to simplex noise of the pixel position by default', () => {
            const { video } = exec(`$v.out($v.noise(4, $v.time));`);
            expect(video!.wgsl).toContain('fn noise_simplex(');
            expect(video!.wgsl).not.toContain('fn noise_value(');
            expect(video!.wgsl).toMatch(
                /noise_simplex\(vec3f\(video_frame_point\(vec2f\(video_luma\(v\d\), video_luma\(v\d\)\)\) \* 4\.0, u\.time\)\)/,
            );
        });

        it('keeps the default position in proportion, measured from the center in frame heights', () => {
            const { wgsl } = exec(`$v.out($v.noise());`).video!;
            expect(wgsl).toContain(
                'return (p - vec2f(0.5)) * vec2f(u.resolution.x / u.resolution.y, 1.0);',
            );
            expect(wgsl).toContain('video_frame_point(vec2f(');
            expect(
                exec(`$v.out($v.noise(5, 0, { mapX: $v.ramp() }));`).video!
                    .wgsl,
            ).not.toContain('video_frame_point');
        });

        it('reads given coordinates times the scale, with z 0 by default', () => {
            const { video } = exec(
                `$v.out($v.hsv($v.noise(2, 0, { mapX: 0.5, mapY: 0.25 })));`,
            );
            expect(video!.wgsl).toContain(
                'noise_simplex(vec3f(vec2f(0.5, 0.25) * 2.0, 0.0))',
            );
        });

        it('compiles each type to its own algorithm and only the helpers it needs', () => {
            const wgslOf = (type: string) =>
                exec(
                    `$v.out($v.noise(1, 0, { type: '${type}', mapX: 0.5, mapY: 0.5 }));`,
                ).video!.wgsl;
            expect(wgslOf('value')).toContain('noise_value(vec3f(');
            expect(wgslOf('value')).not.toContain('fn noise_grad(');
            expect(wgslOf('perlin')).toContain('noise_perlin(vec3f(');
            expect(wgslOf('perlin')).not.toContain('fn noise_simplex(');
            expect(wgslOf('fbm')).toContain('noise_fbm(vec3f(');
            expect(wgslOf('ridged')).toContain('noise_ridged(vec3f(');
        });

        it('rejects an unknown type', () => {
            expect(() =>
                exec(
                    `$v.out($v.noise(1, 0, { type: 'pink', mapX: 0, mapY: 0 }));`,
                ),
            ).toThrow(
                /param "type" must be one of simplex, perlin, value, fbm, ridged/,
            );
        });

        it('rejects a coordinate that is not a signal or a number', () => {
            expect(() => exec(`$v.noise(1, 0, { mapX: 'left' });`)).toThrow(
                /\$v\.noise: mapX must be a number, a video signal or an audio signal/,
            );
        });
    });

    describe('voronoi, polygon and color ops', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('shares the point hash between noise and voronoi', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.add($v.noise(1), $v.voronoi(3, $v.time))));
            `);
            expect(wgsl.match(/fn noise_hash\(/g)).toHaveLength(1);
            expect(wgsl).toContain('fn voronoi_distance(');
            expect(wgsl).toMatch(
                /voronoi_distance\(video_frame_point\(vec2f\(video_luma\(v\d\), video_luma\(v\d\)\)\) \* 3\.0, u\.time\)/,
            );
        });

        it('shapes the edge by the falloff curve, linear by default', () => {
            const linear = wgslOf(`$v.out($v.shape(0.3, 0.2));`);
            expect(linear).toMatch(
                /let v\d: vec4f = vec4f\(vec3f\(shape_edge\(/,
            );
            expect(linear).not.toContain('shape_falloff_');
            const smooth = wgslOf(
                `$v.out($v.polygon(5, 0.3, 0.2, { falloff: 'smooth' }));`,
            );
            expect(smooth).toContain(
                'fn shape_falloff_smooth(t: f32) -> f32 {\n    return t * t * (3.0 - 2.0 * t);',
            );
            expect(smooth).toContain(
                'shape_falloff_smooth(shape_edge(polygon_distance(',
            );
            expect(() =>
                exec(`$v.shape(0.3, 0.2, { falloff: 'cubic' }).out();`),
            ).toThrow(
                /param "falloff" must be one of linear, smooth, smoother, in, out/,
            );
        });

        it('shares the edge function between shape and polygon', () => {
            const wgsl = wgslOf(`
                const a = $v.shape(0.2);
                const b = $v.polygon(5, 0.2);
                $v.out($v.hsv($v.max(a, b)));
            `);
            expect(wgsl.match(/fn shape_edge\(/g)).toHaveLength(1);
            expect(wgsl).toContain('polygon_distance(');
        });

        it('turns hue with the shared hsv helper', () => {
            const wgsl = wgslOf(`
                $v.hsv($v.ramp()).$.hue(0.25).$.contrast(2).out();
            `);
            expect(wgsl.match(/fn hsv_to_rgb\(/g)).toHaveLength(1);
            expect(wgsl).toContain('fn rgb_to_hsv(');
            expect(wgsl).toContain('video_hue(');
            expect(wgsl).toMatch(/video_contrast\(v\d, 2\.0\)/);
        });

        it('adjusts a gray signal or a number like any color', () => {
            expect(() => exec(`$v.out($v.hue($v.ramp(), 0.3));`)).not.toThrow();
            expect(wgslOf(`$v.out($v.contrast(0.4, 2));`)).toContain(
                'video_contrast(vec4f(vec3f(0.4), 1.0), 2.0)',
            );
            expect(() => exec(`$v.contrast('x');`)).toThrow(
                /\$v\.contrast: input must be a number, a video signal or an audio signal/,
            );
        });
    });

    describe('filters', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('blur reads its whole input at sixteen nearby coordinates', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.blur($v.noise(1), 0.02)));
            `);
            expect(wgsl).toMatch(/let v\d+: vec4f = \(node\d+\(uv/);
            expect(wgsl.match(/node\d+\(uv \+ blur_offset\(/g)).toHaveLength(
                16,
            );
            expect(wgsl).toContain('fn blur_offset(');
        });

        it('edges reads the brightness of its input and gives a gray', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.edges($v.hsv($v.ramp()), 3)));
            `);
            expect(wgsl).toContain('video_luma(node');
            expect(wgsl).toMatch(
                /let v\d+: vec4f = vec4f\(vec3f\(clamp\(sobel_magnitude\(/,
            );
        });

        it('edges reads the eight neighbors one pixel away', () => {
            const wgsl = wgslOf(`$v.edges($v.ramp()).$.hsv().out();`);
            expect(wgsl.match(/\/ u\.resolution\)/g)).toHaveLength(8);
        });

        it('chains', () => {
            const wgsl = wgslOf(`
                $v.noise(1).$.blur(0.01).$.edges(2).$.hsv().out();
            `);
            expect(wgsl).toContain('sobel_magnitude(');
        });

        it('rejects a number or a missing signal', () => {
            expect(() => exec(`$v.blur(0.5);`)).toThrow(
                /\$v\.blur: input must be a video signal/,
            );
            expect(() => exec(`$v.edges(0.5);`)).toThrow(
                /\$v\.edges: input must be a video signal/,
            );
        });
    });

    describe('post effects', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('scanlines dims by the frame row', () => {
            const wgsl = wgslOf(
                `$v.out($v.scanlines($v.hsv($v.ramp()), 100, 0.5));`,
            );
            expect(wgsl).toContain('cos(6.28318530718 * uv.y * count)');
            expect(wgsl).toMatch(/video_scanlines\(v\d, uv, 100\.0, 0\.5\)/);
        });

        it('vignette darkens by distance from the center', () => {
            const wgsl = wgslOf(
                `$v.out($v.vignette($v.hsv($v.ramp()), 0.8, 0.2));`,
            );
            expect(wgsl).toContain('fn vignette_falloff(');
            expect(wgsl).toContain('vignette_falloff(uv, 0.2)');
        });

        it('grain is redrawn with the clock and shares the point hash', () => {
            const wgsl = wgslOf(`
                $v.out($v.grain($v.hsv($v.noise(1)), 0.2));
            `);
            expect(wgsl).toContain('grain_value(uv, u.time)');
            expect(wgsl.match(/fn noise_hash\(/g)).toHaveLength(1);
        });

        it('chains in any order and keeps the color', () => {
            const wgsl = wgslOf(`
                $v.hsv($v.ramp()).$.scanlines(180, 0.35).$.vignette(0.7).$.grain(0.08).out();
            `);
            expect(wgsl).toContain('vignette_falloff(');
            expect(wgsl).toContain('grain_value(');
        });

        it('darkens the color and keeps the alpha', () => {
            const wgsl = wgslOf(`$v.out($v.vignette($v.image('a.png')));`);
            expect(wgsl).toContain(
                'return vec4f(c.rgb * (1.0 - amount), c.a);',
            );
        });
    });

    describe('bloom', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('adds a blurred copy back on top of the input', () => {
            const bloomed = wgslOf(`
                $v.out($v.bloom($v.hsv(0.1, 1, $v.shape(0.2)), 0.05, 2));
            `);
            const long = wgslOf(`
                const c = $v.hsv(0.1, 1, $v.shape(0.2));
                $v.out($v.add(c, $v.mult($v.blur(c, 0.05), 2)));
            `);
            expect(bloomed).toBe(long);
        });

        it('adds the glow with the add helper', () => {
            const wgsl = wgslOf(`
                $v.ramp().$.bloom(0.02).$.hsv().out();
            `);
            expect(wgsl).toMatch(/let v\d+: vec4f = video_add\(v\d+, v\d+\);/);
        });

        it('rejects a number', () => {
            expect(() => exec(`$v.bloom(0.5);`)).toThrow(
                /\$v\.bloom: input must be a video signal/,
            );
        });
    });
});

describe('Hydra color adjustments', () => {
    const wgslOf = (source: string) => exec(source).video!.wgsl;
    const near = (actual: unknown, expected: number[]) => {
        (actual as number[]).forEach((v, k) =>
            expect(v).toBeCloseTo(expected[k], 5),
        );
    };

    it('brightness adds to every color channel without clipping', () => {
        const wgsl = wgslOf(`$v.osc(4).$.brightness().out()`);
        expect(wgsl).toMatch(/video_brightness\(v\d, 0\.4\)/);
        near(
            callWgsl(wgsl, 'video_brightness', [0.2, 0.5, 0.9, 0.7], 0.4),
            [0.6, 0.9, 1.3, 0.7],
        );
    });

    it("saturate mixes away from the gray with Hydra's weights", () => {
        const wgsl = wgslOf(`$v.osc(4).$.saturate().out()`);
        expect(wgsl).toMatch(/video_saturate\(v\d, 2\.0\)/);
        const gray = 0.2125 * 0.8 + 0.7154 * 0.4 + 0.0721 * 0.2;
        near(callWgsl(wgsl, 'video_saturate', [0.8, 0.4, 0.2, 1], 0), [
            gray,
            gray,
            gray,
            1,
        ]);
        near(callWgsl(wgsl, 'video_saturate', [0.8, 0.4, 0.2, 0.5], 2), [
            2 * 0.8 - gray,
            2 * 0.4 - gray,
            2 * 0.2 - gray,
            0.5,
        ]);
    });

    it('colorama shifts hue, saturation and value and wraps each channel', () => {
        const wgsl = wgslOf(`$v.osc(4).$.hsv().$.colorama().out()`);
        expect(wgsl).toMatch(/video_colorama\(v\d, 0\.005\)/);
        // Pure red is hue 0, saturation 1, value 1; adding 0.5 gives hue 0.5
        // (cyan), saturation 1.5 and value 1.5: (-0.75, 1.5, 1.5) before
        // wrapping.
        near(
            callWgsl(wgsl, 'video_colorama', [1, 0, 0, 0.8], 0.5),
            [0.25, 0.5, 0.5, 0.8],
        );
        near(callWgsl(wgsl, 'video_colorama', [1, 0, 0, 1], 0), [0, 0, 0, 1]);
    });

    it('chains under .$ and .$m', () => {
        expect(() =>
            exec(
                `$v.osc(4).$.brightness(0.1).$.saturate(1.5).$m.colorama(0.5, 0.1).out()`,
            ),
        ).not.toThrow();
    });
});

describe('Hydra color functions', () => {
    const wgslOf = (source: string) => exec(source).video!.wgsl;
    const near = (actual: unknown, expected: number[]) => {
        (actual as number[]).forEach((v, k) =>
            expect(v).toBeCloseTo(expected[k], 5),
        );
    };

    it('add sums every channel, alpha included, past 1', () => {
        const wgsl = wgslOf(`$v.add($v.osc(4), $v.osc(2)).out()`);
        near(
            callWgsl(wgsl, 'video_add', [0.8, 0.6, 0.4, 1], [0.5, 0, 0.2, 1]),
            [1.3, 0.6, 0.6, 2],
        );
    });

    it('contrast scales the distance from mid-gray past 0 and 1', () => {
        const wgsl = wgslOf(`$v.osc(4).$.contrast(3).out()`);
        near(
            callWgsl(wgsl, 'video_contrast', [0.9, 0.1, 0.5, 0.7], 3),
            [1.7, -0.7, 0.5, 0.7],
        );
    });

    it('color multiplies each channel, and a negative amount inverts it', () => {
        const wgsl = wgslOf(`$v.osc(4).$.color(1, 0.5, 0).out()`);
        expect(wgsl).toMatch(
            /video_color\(v\d, vec4f\(1\.0, 0\.5, 0\.0, 1\.0\)\)/,
        );
        near(
            callWgsl(wgsl, 'video_color', [0.8, 0.6, 0.4, 1], [1, 0.5, 0, 1]),
            [0.8, 0.3, 0, 1],
        );
        near(
            callWgsl(wgsl, 'video_color', [0.8, 0.6, 0.4, 1], [-1, 1, -0.5, 1]),
            [0.2, 0.6, 0.3, 1],
        );
    });

    it('shift adds the fractional part of each amount without wrapping', () => {
        const wgsl = wgslOf(`$v.osc(4).$.shift().out()`);
        expect(wgsl).toMatch(
            /video_shift\(v\d, vec4f\(0\.5, 0\.0, 0\.0, 0\.0\)\)/,
        );
        near(
            callWgsl(
                wgsl,
                'video_shift',
                [0.8, 0.2, 0.1, 1],
                [0.5, 1.25, 0, 0],
            ),
            [1.3, 0.45, 0.1, 1],
        );
    });

    it('thresh steps the brightness into a gray and keeps the alpha', () => {
        const wgsl = wgslOf(`$v.osc(4).$.thresh().out()`);
        expect(wgsl).toMatch(/video_thresh\(v\d, 0\.5, 0\.04\)/);
        near(
            callWgsl(wgsl, 'video_thresh', [0.6, 0.6, 0.6, 0.5], 0.5, 0.04),
            [1, 1, 1, 0.5],
        );
        near(
            callWgsl(wgsl, 'video_thresh', [0.4, 0.4, 0.4, 1], 0.5, 0.04),
            [0, 0, 0, 1],
        );
        near(
            callWgsl(wgsl, 'video_thresh', [0.5, 0.5, 0.5, 1], 0.5, 0.04),
            [0.5, 0.5, 0.5, 1],
        );
    });

    it('invert crossfades toward the complement by amount, unclipped', () => {
        const wgsl = wgslOf(`$v.osc(4).$.invert(0.25).out()`);
        expect(wgsl).toMatch(/video_invert\(v\d, 0\.25\)/);
        near(
            callWgsl(wgsl, 'video_invert', [0.8, 0.2, 1.5, 0.6], 1),
            [0.2, 0.8, -0.5, 0.6],
        );
        near(
            callWgsl(wgsl, 'video_invert', [0.8, 0.2, 0, 1], 0.5),
            [0.5, 0.5, 0.5, 1],
        );
    });

    it("takes Hydra's defaults for hue and solid", () => {
        expect(wgslOf(`$v.osc(4).$.hsv().$.hue().out()`)).toMatch(
            /video_hue\(v\d, 0\.4\)/,
        );
        expect(wgslOf(`$v.solid().out()`)).toContain(
            'vec4f(0.0, 0.0, 0.0, 1.0)',
        );
    });
});
