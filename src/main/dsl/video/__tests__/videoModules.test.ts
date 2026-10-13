import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';

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

        it('gives a number to a signal operand as an opaque gray', () => {
            const wgsl = wgslOf(`
                $v.out($v.add($v.colorize(1, 0, 0), 0.25));
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
                    `$v.out($v.hsv($v.shape(0.2, 0, { shape: 'star', x: 0.5, y: 0.5 })));`,
                ),
            ).toThrow(/param "shape" must be one of circle, box, diamond/);
        });
    });

    describe('coordinate ramps and waveshaping', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('compiles an untransformed ramp to the bare frame coordinate', () => {
            const wgsl = wgslOf(`$v.out($v.colorize($v.ramp(), 0, 0));`);
            expect(wgsl).toContain('let v0: vec4f = vec4f(vec3f(uv.x), 1.0);');
        });

        it('routes a transformed ramp through video_transform', () => {
            const wgsl = wgslOf(
                `$v.out($v.colorize($v.ramp('vertical', { rotate: 0.1, zoom: 2 }), 0, 0));`,
            );
            expect(wgsl).toContain(
                'video_transform(uv, 2.0, 0.1, vec2f(0.0, 0.0)).y',
            );
        });

        it('offers radial and angular ramps', () => {
            const wgsl = wgslOf(
                `$v.out($v.colorize($v.ramp('radial'), $v.ramp('angular'), 0));`,
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
                $v.out($v.colorize(
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
            expect(wgsl).toMatch(/video_posterize\(v\d, 3\.0\)/);
            expect(wgsl).toMatch(/video_comparator\(v\d, 0\.4, 0\.0\)/);
            expect(wgsl).toContain('return vec4f(clamp((x - vec3f(threshold))');
        });

        it('rejects an unknown ramp axis', () => {
            expect(() =>
                exec(`$v.out($v.colorize($v.ramp('q'), 0, 0));`),
            ).toThrow(
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
                /noise_simplex\(vec3f\(vec2f\(video_luma\(v\d\), video_luma\(v\d\)\) \* 4\.0, u\.time\)\)/,
            );
        });

        it('reads given coordinates times the scale, with z 0 by default', () => {
            const { video } = exec(
                `$v.out($v.hsv($v.noise(2, 0, { x: 0.5, y: 0.25 })));`,
            );
            expect(video!.wgsl).toContain(
                'noise_simplex(vec3f(vec2f(0.5, 0.25) * 2.0, 0.0))',
            );
        });

        it('compiles each type to its own algorithm and only the helpers it needs', () => {
            const wgslOf = (type: string) =>
                exec(
                    `$v.out($v.noise(1, 0, { type: '${type}', x: 0.5, y: 0.5 }));`,
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
                exec(`$v.out($v.noise(1, 0, { type: 'pink', x: 0, y: 0 }));`),
            ).toThrow(
                /param "type" must be one of simplex, perlin, value, fbm, ridged/,
            );
        });

        it('rejects a coordinate that is not a signal or a number', () => {
            expect(() => exec(`$v.noise(1, 0, { x: 'left' });`)).toThrow(
                /\$v\.noise: x must be a number, a video signal or an audio signal/,
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
                /voronoi_distance\(vec2f\(video_luma\(v\d\), video_luma\(v\d\)\) \* 3\.0, u\.time\)/,
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
                $v.hsv($v.ramp()).$.hueShift(0.25).$.contrast(2).out();
            `);
            expect(wgsl.match(/fn hsv_to_rgb\(/g)).toHaveLength(1);
            expect(wgsl).toContain('fn rgb_to_hsv(');
            expect(wgsl).toContain('hue_shift(');
            expect(wgsl).toMatch(/video_contrast\(v\d, 2\.0\)/);
        });

        it('adjusts a gray signal or a number like any color', () => {
            expect(() =>
                exec(`$v.out($v.hueShift($v.ramp(), 0.3));`),
            ).not.toThrow();
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
            expect(wgsl).toMatch(/let v\d+: vec4f = \(f\d+\(uv/);
            expect(wgsl.match(/f\d+\(uv \+ blur_offset\(/g)).toHaveLength(16);
            expect(wgsl).toContain('fn blur_offset(');
        });

        it('edges reads the brightness of its input and gives a gray', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.edges($v.hsv($v.ramp()), 3)));
            `);
            expect(wgsl).toContain('video_luma(f');
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
