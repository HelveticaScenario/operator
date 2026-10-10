import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';

describe('$v modules', () => {
    describe('math and keying modules', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('uses the field variant for fields and the color variant for colors', () => {
            const fields = wgslOf(
                `$v.out($v.colorize($v.mult($v.ramp(), 2.5), 0, 0));`,
            );
            expect(fields).toContain('let v1: f32 = v0 * 0.5;');
            const colors = wgslOf(`
                const c = $v.hsv($v.ramp());
                $v.out($v.mult(c, $v.ramp('v')));
            `);
            expect(colors).toMatch(/let v\d: vec3f = v\d \* v\d;/);
        });

        it('promotes a field operand of a color operation to gray', () => {
            const wgsl = wgslOf(`
                $v.out($v.add($v.colorize(5, 0, 0), 1.25));
            `);
            expect(wgsl).toContain('vec3f(0.25, 0.25, 0.25)');
        });

        it('declares each module helper once however often the module is used', () => {
            const wgsl = wgslOf(`
                const a = $v.shape($v.ramp(), $v.ramp('v'), 0.2);
                const b = $v.shape($v.ramp(), $v.ramp('v'), 0.1, 0, { shape: 'box' });
                $v.out($v.key($v.hsv(a), $v.procAmp($v.hsv(b)), $v.comparator(a)));
            `);
            expect(wgsl.match(/fn shape_edge/g)).toHaveLength(1);
            expect(wgsl.match(/fn hsv_to_rgb/g)).toHaveLength(1);
            expect(wgsl).toContain('fn procamp');
        });

        it('rejects a color where a field is required', () => {
            expect(() => exec(`$v.comparator($v.colorize(0, 0, 0));`)).toThrow(
                /\$v\.comparator: input must be a number or a video field/,
            );
            expect(() =>
                exec(`$v.mix($v.ramp(), $v.ramp(), $v.colorize(0, 0, 0));`),
            ).toThrow(/\$v\.mix: amount must be a number or a video field/);
        });

        it('rejects an invalid shape option', () => {
            expect(() =>
                exec(
                    `$v.out($v.hsv($v.shape(0.5, 0.5, 0.2, 0, { shape: 'star' })));`,
                ),
            ).toThrow(/param "shape" must be one of circle, box, diamond/);
        });
    });

    describe('coordinate ramps and waveshaping', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('compiles an untransformed ramp to the bare frame coordinate', () => {
            const wgsl = wgslOf(`$v.out($v.colorize($v.ramp(), 0, 0));`);
            expect(wgsl).toContain('let v0: f32 = uv.x;');
        });

        it('routes a transformed ramp through video_transform', () => {
            const wgsl = wgslOf(
                `$v.out($v.colorize($v.ramp('v', { rotate: 0.5, zoom: 2 }), 0, 0));`,
            );
            expect(wgsl).toContain(
                'video_transform(uv, 2.0, 0.1, vec2f(0.0, 0.0)).y',
            );
        });

        it('offers radial and angular ramps', () => {
            const wgsl = wgslOf(
                `$v.out($v.colorize($v.ramp('r'), $v.ramp('a'), 0));`,
            );
            expect(wgsl).toContain('ramp_radius(uv)');
            expect(wgsl).toContain('ramp_angle(uv)');
        });

        it('declares the shared transform once when ramps and feedback both use it', () => {
            const wgsl = wgslOf(`
                const r = $v.ramp('h', { rotate: 0.1 });
                $v.out($v.feedback((prev) => $v.mix($v.hsv(r), prev, 0.9), { rotate: 0.01 }));
            `);
            expect(wgsl.match(/fn video_transform/g)).toHaveLength(1);
        });

        it('lets a slider drive a ramp transform', () => {
            const { video } = exec(`
                const spin = $slider('Spin', 0, -0.5, 0.5);
                $v.out($v.hsv($v.ramp('a', { rotate: spin })));
            `);
            expect(video!.uniforms).toHaveLength(1);
        });

        it('compiles min, max, wrap and fold', () => {
            const wgsl = wgslOf(`
                const x = $v.ramp();
                const y = $v.ramp('v');
                $v.out($v.colorize(
                    $v.max($v.wrap(x, 3), $v.fold(y, 4)),
                    $v.min(x, y),
                    0,
                ));
            `);
            expect(wgsl).toContain('fract(');
            expect(wgsl).toMatch(/max\(v\d, v\d\)/);
            expect(wgsl).toMatch(/min\(v\d, v\d\)/);
        });

        it('applies min and max per channel to colors', () => {
            const wgsl = wgslOf(
                `$v.out($v.max($v.hsv($v.ramp()), $v.hsv($v.ramp('v'))));`,
            );
            expect(wgsl).toMatch(/let v\d: vec3f = max\(v\d, v\d\);/);
        });

        it('rejects an unknown ramp axis', () => {
            expect(() =>
                exec(`$v.out($v.colorize($v.ramp('q'), 0, 0));`),
            ).toThrow(/param "axis" must be one of h, v, d, r, a/);
        });
    });

    describe('noise', () => {
        it('compiles to value noise of its three coordinates', () => {
            const { video } = exec(`
                $v.out($v.hsv($v.noise($v.mult($v.ramp(), 4), $v.ramp('v'), $v.time)));
            `);
            expect(video!.wgsl).toContain('fn noise_hash(');
            expect(video!.wgsl).toContain('fn noise_value(');
            expect(video!.wgsl).toMatch(
                /noise_value\(vec3f\(\(v\d \* 5\.0\), \(v\d \* 5\.0\), u\.time\)\)/,
            );
        });

        it('takes a constant z by default', () => {
            const { video } = exec(`$v.out($v.hsv($v.noise(0.5, 0.5)));`);
            expect(video!.wgsl).toContain('noise_value(vec3f(0.5, 0.5, 0.0))');
        });

        it('rejects a color coordinate', () => {
            expect(() => exec(`$v.noise($v.hsv(0), 0);`)).toThrow(
                /\$v\.noise: x must be a number or a video field/,
            );
        });
    });

    describe('voronoi, polygon and color ops', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('shares the point hash between noise and voronoi', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.add($v.noise($v.ramp(), $v.ramp('v')), $v.voronoi($v.ramp(), $v.ramp('v'), $v.time))));
            `);
            expect(wgsl.match(/fn noise_hash\(/g)).toHaveLength(1);
            expect(wgsl).toContain('fn voronoi_distance(');
            expect(wgsl).toMatch(
                /voronoi_distance\(vec2f\(\(v\d \* 5\.0\), \(v\d \* 5\.0\)\), u\.time\)/,
            );
        });

        it('shares the edge function between shape and polygon', () => {
            const wgsl = wgslOf(`
                const a = $v.shape($v.ramp(), $v.ramp('v'), 0.2);
                const b = $v.polygon($v.ramp(), $v.ramp('v'), 5, 0.2);
                $v.out($v.hsv($v.max(a, b)));
            `);
            expect(wgsl.match(/fn shape_edge\(/g)).toHaveLength(1);
            expect(wgsl).toContain('polygon_distance(');
        });

        it('turns hue with the shared hsv helper', () => {
            const wgsl = wgslOf(`
                $v.hsv($v.ramp()).$.hueShift(1.25).$.contrast(10).out();
            `);
            expect(wgsl.match(/fn hsv_to_rgb\(/g)).toHaveLength(1);
            expect(wgsl).toContain('fn rgb_to_hsv(');
            expect(wgsl).toContain('hue_shift(');
            expect(wgsl).toMatch(/\* 2\.0 \+ vec3f\(0\.5\)/);
        });

        it('treats a field or number as a gray color', () => {
            const gray = `
                const r = $v.ramp();
                const g = $v.colorize(r, r, r);
            `;
            expect(wgslOf(`$v.out($v.hueShift($v.ramp(), 0.3));`)).toBe(
                wgslOf(`${gray} $v.out($v.hueShift(g, 0.3));`),
            );
            expect(wgslOf(`$v.out($v.contrast($v.ramp(), 2));`)).toBe(
                wgslOf(`${gray} $v.out($v.contrast(g, 2));`),
            );
            expect(() => exec(`$v.contrast('x');`)).toThrow(
                /\$v\.contrast: input must be a number or a video field/,
            );
        });
    });

    describe('filters', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('blur reads its whole input at sixteen nearby coordinates', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.blur($v.noise($v.ramp(), $v.ramp('v')), 0.02)));
            `);
            const returned = wgsl.match(/let v\d+: f32 = \(f\d+\(uv/);
            expect(returned).not.toBeNull();
            expect(wgsl.match(/f\d+\(uv \+ blur_offset\(/g)).toHaveLength(16);
            expect(wgsl).toContain('fn blur_offset(');
        });

        it('blur keeps a color a color', () => {
            const wgsl = wgslOf(`$v.hsv($v.ramp()).$.blur(0.05).out();`);
            expect(wgsl).toMatch(/let v\d+: vec3f = \(f\d+\(uv/);
        });

        it('edges is a field whatever it reads', () => {
            const fromField = wgslOf(`
                $v.out($v.hsv($v.edges($v.noise($v.ramp(), $v.ramp('v')), 3)));
            `);
            expect(fromField).toMatch(
                /let v\d+: f32 = clamp\(sobel_magnitude\(/,
            );
            const fromColor = wgslOf(`
                $v.out($v.hsv($v.edges($v.hsv($v.ramp()))));
            `);
            expect(fromColor).toContain('dot(f');
            expect(fromColor).toMatch(
                /let v\d+: f32 = clamp\(sobel_magnitude\(/,
            );
        });

        it('edges reads the eight neighbors one pixel away', () => {
            const wgsl = wgslOf(`$v.edges($v.ramp()).$.hsv().out();`);
            expect(wgsl.match(/\/ u\.resolution\)/g)).toHaveLength(8);
        });

        it('chains', () => {
            const wgsl = wgslOf(`
                $v.noise($v.ramp(), $v.ramp('v')).$.blur(0.01).$.edges(2).$.hsv().out();
            `);
            expect(wgsl).toContain('sobel_magnitude(');
        });

        it('rejects a number or a missing signal', () => {
            expect(() => exec(`$v.blur(0.5);`)).toThrow(
                /\$v\.blur: input must be a video field or color/,
            );
            expect(() => exec(`$v.edges(0.5);`)).toThrow(
                /\$v\.edges: input must be a video field or color/,
            );
        });
    });

    describe('post effects', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('scanlines dims by the frame row', () => {
            const wgsl = wgslOf(
                `$v.out($v.scanlines($v.hsv($v.ramp()), 100, 0.5));`,
            );
            expect(wgsl).toMatch(/cos\(6\.28318530718 \* uv\.y \* 100\.0\)/);
        });

        it('vignette darkens by distance from the center', () => {
            const wgsl = wgslOf(
                `$v.out($v.vignette($v.hsv($v.ramp()), 4, 1));`,
            );
            expect(wgsl).toContain('fn vignette_falloff(');
            expect(wgsl).toContain('vignette_falloff(uv, 0.2)');
        });

        it('grain is redrawn with the clock and shares the point hash', () => {
            const wgsl = wgslOf(`
                $v.out($v.grain($v.hsv($v.noise($v.ramp(), $v.ramp('v'))), 0.2));
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

        it('treats a field as a gray color', () => {
            const gray = `
                const r = $v.ramp();
                const g = $v.colorize(r, r, r);
            `;
            for (const fn of ['scanlines', 'vignette', 'grain']) {
                expect(wgslOf(`$v.out($v.${fn}($v.ramp()));`)).toBe(
                    wgslOf(`${gray} $v.out($v.${fn}(g));`),
                );
            }
        });
    });

    describe('bloom', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('adds a blurred copy back on top of the input', () => {
            const bloomed = wgslOf(`
                $v.out($v.bloom($v.hsv(0.1, 1, $v.shape($v.ramp(), $v.ramp('v'), 0.2)), 0.05, 2));
            `);
            const long = wgslOf(`
                const c = $v.hsv(0.1, 1, $v.shape($v.ramp(), $v.ramp('v'), 0.2));
                $v.out($v.add(c, $v.mult($v.blur(c, 0.05), 2)));
            `);
            expect(bloomed).toBe(long);
        });

        it('keeps a field a field', () => {
            const wgsl = wgslOf(`
                $v.ramp().$.bloom(0.02).$.hsv().out();
            `);
            expect(wgsl).toMatch(
                /let v\d+: f32 = clamp\(v\d+ \+ v\d+, 0\.0, 1\.0\)/,
            );
        });

        it('rejects a number', () => {
            expect(() => exec(`$v.bloom(0.5);`)).toThrow(
                /\$v\.bloom: input must be a video field or color/,
            );
        });
    });
});
