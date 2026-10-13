import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';
import { callWgsl } from './wgslEval';

describe('$v warps and modulators', () => {
    describe('coordinate warps', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('re-evaluates the whole input at the moved coordinates', () => {
            const wgsl = wgslOf(`
                const grain = $v.noise(4);
                $v.out($v.hsv($v.warp(grain, { rotate: 0.1 })));
            `);
            // The noise and both of its coordinates are functions of the
            // coordinate; the warp calls the noise at the moved one.
            expect(wgsl).toContain(
                'fn f2(uv: vec2f) -> vec4f {\n    return vec4f(vec3f(clamp((0.5 + 0.5 * noise_simplex(vec3f(video_frame_point(vec2f(video_luma(f0(uv)), video_luma(f1(uv)))) * 4.0, 0.0))), 0.0, 1.0)), 1.0);',
            );
            expect(wgsl).toContain(
                'let v3: vec4f = f2(video_transform(uv, 1.0, 0.1, vec2f(0.0, 0.0)));',
            );
        });

        it('evaluates the input at coordinates, not a precomputed value', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.kaleid($v.osc(3), 6)));
            `);
            expect(wgsl).toContain('fn f1(uv: vec2f) -> vec4f');
            expect(wgsl).toContain('f0(uv)');
            expect(wgsl).toContain('video_kaleid(uv, 6.0, 0.0)');
        });

        it('nests: a warp inside a warp is itself a function of the coordinate', () => {
            const wgsl = wgslOf(`
                const inner = $v.pixelate($v.noise(1), 10);
                $v.out($v.hsv($v.kaleid(inner, 5)));
            `);
            // The outer transform calls the inner one as a function, and the
            // inner one calls the noise as a function in turn.
            expect(wgsl).toMatch(
                /fn f3\(uv: vec2f\) -> vec4f \{\n    return f2\(video_pixelate/,
            );
        });

        it('warps a constant or time input through a constant function', () => {
            const wgsl = wgslOf(`$v.out($v.hsv($v.warp($v.time)));`);
            expect(wgsl).toMatch(
                /fn k0\(uv: vec2f\) -> vec4f \{\n    return vec4f\(vec3f\(u\.time\), 1\.0\);/,
            );
            expect(wgsl).toContain('k0(video_transform(');
        });

        it('keeps a feedback loop working inside a warp', () => {
            const { video } = exec(`
                const trail = $v.feedback((prev) => $v.mix($v.hsv(0.2, 1, $v.shape(0.1)), prev, 0.9));
                $v.out($v.warp(trail, { rotate: 0.05 }));
            `);
            expect(video!.feedbackBufferCount).toBe(1);
            expect(video!.wgsl).toContain('textureSampleLevel(fb_0');
        });

        it('moves a color the same way as a gray signal', () => {
            const wgsl = wgslOf(`
                $v.out($v.pixelate($v.hsv($v.ramp()), 8, 4));
            `);
            expect(wgsl).toMatch(/let v\d: vec4f = f\d\(video_pixelate/);
        });

        it('pulls a channel or the brightness out of a signal', () => {
            const wgsl = wgslOf(`
                $v.out($v.colorize($v.channel($v.hsv($v.ramp()), 'g'), $v.channel($v.hsv($v.ramp())), 0));
            `);
            expect(wgsl).toMatch(/vec4f\(vec3f\(v\d+\.g\), 1\.0\)/);
            expect(wgsl).toMatch(/vec4f\(vec3f\(video_luma\(v\d+\)\), 1\.0\)/);
        });

        it('rejects a number to warp or an unknown channel', () => {
            expect(() => exec(`$v.warp(0.5);`)).toThrow(
                /\$v\.warp: input must be a video signal/,
            );
            expect(() =>
                exec(
                    `$v.out($v.colorize($v.channel($v.hsv(0), 'alpha'), 0, 0));`,
                ),
            ).toThrow(/param "channel" must be one of r, g, b, a, luma/);
        });
    });

    describe('modulators', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;
        const names = [
            ['modulate', 'uv + v'],
            ['modulateScale', 'video_mod_scale('],
            ['modulateRotate', 'video_transform(uv, 1.0,'],
            ['modulatePixelate', 'video_pixelate(uv, vec2f('],
            ['modulateKaleid', 'video_kaleid('],
            ['modulateHue', 'video_mod_hue('],
            ['modulateRepeat', 'video_mod_repeat('],
            ['modulateRepeatX', 'video_mod_repeat_x('],
            ['modulateRepeatY', 'video_mod_repeat_y('],
            ['modulateScrollX', 'fract(uv + vec2f('],
            ['modulateScrollY', 'fract(uv + vec2f('],
        ] as const;
        const noise = `$v.noise(1)`;

        it.each(names)(
            '%s moves a gray signal by a gray signal',
            (name, emitted) => {
                const wgsl = wgslOf(
                    `$v.out($v.hsv($v.${name}($v.osc(4), ${noise})));`,
                );
                expect(wgsl).toContain(emitted);
                expect(wgsl).toMatch(/let v\d+: vec4f = f\d+\(/);
            },
        );

        it.each(names)('%s moves a color by a color', (name, emitted) => {
            const wgsl = wgslOf(
                `$v.out($v.${name}($v.hsv($v.ramp()), $v.hsv(${noise})));`,
            );
            expect(wgsl).toContain(emitted);
            expect(wgsl).toMatch(/let v\d+: vec4f = f\d+\(/);
        });

        it('reads the modulator whole and takes its channels', () => {
            const wgsl = wgslOf(
                `$v.out($v.modulateScale($v.hsv($v.ramp()), $v.hsv(${noise})));`,
            );
            expect(wgsl).toMatch(/video_mod_scale\(uv, v\d+\.rg, 1\.0, 1\.0\)/);
        });

        it('mirrors through the center for a negative scale and keeps it off zero', () => {
            const wgsl = wgslOf(
                `$v.out($v.modulateScale($v.hsv($v.ramp()), $v.hsv(${noise})));`,
            );
            const scaled = (factor: number) =>
                (
                    callWgsl(
                        wgsl,
                        'video_mod_scale',
                        [0.75, 0.5],
                        [0, 0],
                        1,
                        factor,
                    ) as number[]
                ).map((x) => Number(x.toFixed(6)));
            expect(scaled(2)).toEqual([0.625, 0.5]);
            expect(scaled(-2)).toEqual([0.375, 0.5]);
            expect(scaled(0)[0]).toBeGreaterThan(1000);
            expect(scaled(-1e-9)[0]).toBeLessThan(-1000);
        });

        it('chains the way the function is called', () => {
            const chained = wgslOf(`
                $v.hsv($v.ramp()).$.modulateScale(${noise}, 2, 0.5).out();
            `);
            const called = wgslOf(`
                $v.out($v.modulateScale($v.hsv($v.ramp()), ${noise}, 2, 0.5));
            `);
            expect(chained).toBe(called);
        });

        it('reads its angles in turns and its counts as they are', () => {
            const rotate = wgslOf(
                `$v.out($v.modulateRotate($v.hsv($v.ramp()), ${noise}, 0.5, 1));`,
            );
            expect(rotate).toMatch(
                /video_transform\(uv, 1\.0, 1\.0 \+ v\d+\.r \* 0\.5/,
            );
            const cells = wgslOf(
                `$v.out($v.modulatePixelate($v.hsv($v.ramp()), ${noise}, 12, 4));`,
            );
            expect(cells).toMatch(
                /video_pixelate\(uv, vec2f\(4\.0\) \+ v\d+\.rg \* 12\.0\)/,
            );
        });

        it('pushes by the channels themselves, so black leaves the input in place', () => {
            const wgsl = wgslOf(
                `$v.out($v.modulate($v.hsv($v.ramp()), $v.hsv(${noise}), 0.2));`,
            );
            expect(wgsl).toMatch(/f\d+\(uv \+ v\d+\.rg \* 0\.2\)/);
        });

        it('samples pixelated cells at their centers', () => {
            const wgsl = wgslOf(
                `$v.out($v.modulatePixelate($v.hsv($v.ramp()), ${noise}));`,
            );
            expect(wgsl).toContain('(floor(uv * grid) + vec2f(0.5)) / grid');
        });

        it('slides every repeated row and column alike, as Hydra does', () => {
            const wgsl = wgslOf(
                `$v.out($v.modulateRepeat($v.hsv($v.ramp()), $v.hsv(${noise})));`,
            );
            const repeat = wgsl.match(/fn video_mod_repeat\(.*?\n}/s)?.[0];
            expect(repeat).toContain('video_mod(st.y, 2.0)');
            expect(repeat).not.toContain('%');
            expect(wgsl).toContain('x - m * floor(x / m)');

            const at = (uv: number[]) =>
                (
                    callWgsl(
                        wgsl,
                        'video_mod_repeat',
                        uv,
                        [4, 4],
                        [0.5, 0.25],
                        [0.4, 0.4],
                    ) as number[]
                ).map((x) => Number(x.toFixed(6)));
            const slid = [0.05 * 4 + 0.2, 0.05 * 4 + 0.1].map((x) =>
                Number(x.toFixed(6)),
            );
            for (const cell of [
                [0, 0],
                [0.25, 0],
                [0, 0.25],
                [0.25, 0.25],
                [-0.25, -0.5],
            ]) {
                expect(at([cell[0] + 0.05, cell[1] + 0.05])).toEqual(slid);
            }
        });

        it('rejects a modulator that is not a video signal', () => {
            for (const [name] of names) {
                expect(() => exec(`$v.${name}($v.hsv(0), 'noise');`)).toThrow(
                    new RegExp(
                        `\\$v\\.${name}: modulator must be a video signal`,
                    ),
                );
            }
        });
    });
});
