import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';

describe('$v warps and modulators', () => {
    describe('coordinate warps', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('re-evaluates the whole input at the moved coordinates', () => {
            const wgsl = wgslOf(`
                const grain = $v.noise($v.mult($v.ramp(), 4), $v.mult($v.ramp('v'), 4));
                $v.out($v.hsv($v.warp(grain, { rotate: 0.5 })));
            `);
            // The noise and both of its coordinates are functions of the
            // coordinate; the warp calls the noise at the moved one.
            expect(wgsl).toContain(
                'fn f4(uv: vec2f) -> f32 {\n    return noise_value(vec3f((f1(uv) * 5.0), (f3(uv) * 5.0), 0.0));',
            );
            expect(wgsl).toContain(
                'let v5: f32 = f4(video_transform(uv, 1.0, 0.1, vec2f(0.0, 0.0)));',
            );
        });

        it('evaluates the input at coordinates, not a precomputed value', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.kaleid($v.osc($v.ramp(), 3), 6)));
            `);
            expect(wgsl).toContain('fn f1(uv: vec2f) -> f32');
            expect(wgsl).toContain('f0(uv)');
            expect(wgsl).toContain('video_kaleid(uv, 6.0, 0.0)');
        });

        it('nests: a warp inside a warp is itself a function of the coordinate', () => {
            const wgsl = wgslOf(`
                const inner = $v.pixelate($v.noise($v.ramp(), $v.ramp('v')), 10);
                $v.out($v.hsv($v.kaleid(inner, 5)));
            `);
            // The outer transform calls the inner one as a function, and the
            // inner one calls the noise as a function in turn.
            expect(wgsl).toMatch(
                /fn f3\(uv: vec2f\) -> f32 \{\n    return f2\(video_pixelate/,
            );
        });

        it('warps a constant or time input through a constant function', () => {
            const wgsl = wgslOf(`$v.out($v.hsv($v.warp($v.time)));`);
            expect(wgsl).toMatch(
                /fn k0\(uv: vec2f\) -> f32 \{\n    return \(u\.time \* 0\.2\);/,
            );
            expect(wgsl).toContain('k0(video_transform(');
        });

        it('keeps a feedback loop working inside a warp', () => {
            const { video } = exec(`
                const trail = $v.feedback((prev) => $v.mix($v.hsv(0.2, 1, $v.shape($v.ramp(), $v.ramp('v'), 0.1)), prev, 0.9));
                $v.out($v.warp(trail, { rotate: 0.05 }));
            `);
            expect(video!.feedbackBufferCount).toBe(1);
            expect(video!.wgsl).toContain('textureSampleLevel(fb_0');
        });

        it('chooses the field or color variant from the input', () => {
            const wgsl = wgslOf(`
                $v.out($v.pixelate($v.hsv($v.ramp()), 8, 4));
            `);
            expect(wgsl).toMatch(/let v\d: vec3f = f\d\(video_pixelate/);
        });

        it('modulates by the red and green of a color, or the value of a field', () => {
            const color = wgslOf(`
                const m = $v.hsv($v.ramp());
                $v.out($v.hsv($v.modulate($v.osc($v.ramp(), 4), m, 0.2)));
            `);
            expect(color).toContain('.r;');
            expect(color).toContain('.g;');
            const field = wgslOf(`
                $v.out($v.hsv($v.modulate($v.osc($v.ramp(), 4), $v.ramp('v'), 0.2)));
            `);
            expect(field).not.toContain('.r;');
        });

        it('pulls a channel out of a color', () => {
            const wgsl = wgslOf(`
                $v.out($v.colorize($v.channel($v.hsv($v.ramp()), 'g'), $v.channel($v.hsv($v.ramp())), 0));
            `);
            expect(wgsl).toMatch(/\.g;/);
            expect(wgsl).toContain('dot(');
        });

        it('rejects a number or the wrong kind of signal', () => {
            expect(() => exec(`$v.warp(0.5);`)).toThrow(
                /\$v\.warp: input must be a video field or color/,
            );
            expect(() => exec(`$v.channel(0.5);`)).toThrow(
                /\$v\.channel: input must be a video field or color/,
            );
            expect(() =>
                exec(
                    `$v.out($v.colorize($v.channel($v.hsv(0), 'alpha'), 0, 0));`,
                ),
            ).toThrow(/param "channel" must be one of r, g, b, luma/);
        });
    });

    describe("Hydra's modulators", () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;
        const names = [
            ['modulateScale', 'video_mod_scale('],
            ['modulateRotate', 'video_transform(uv, 1.0,'],
            ['modulatePixelate', 'video_mod_pixelate('],
            ['modulateKaleid', 'video_kaleid('],
            ['modulateHue', 'u.resolution'],
            ['modulateRepeat', 'video_mod_repeat('],
            ['modulateRepeatX', 'video_mod_repeat_x('],
            ['modulateRepeatY', 'video_mod_repeat_y('],
            ['modulateScrollX', 'fract(uv + vec2f('],
            ['modulateScrollY', 'fract(uv + vec2f('],
        ] as const;
        const noise = `$v.noise($v.ramp(), $v.ramp('v'))`;

        it.each(names)('%s moves a field by a field', (name, emitted) => {
            const wgsl = wgslOf(
                `$v.out($v.hsv($v.${name}($v.osc($v.ramp(), 4), ${noise})));`,
            );
            expect(wgsl).toContain(emitted);
            expect(wgsl).toMatch(/let v\d+: f32 = f\d+\(/);
        });

        it.each(names)('%s moves a color by a color', (name, emitted) => {
            const wgsl = wgslOf(
                `$v.out($v.${name}($v.hsv($v.ramp()), $v.hsv(${noise})));`,
            );
            expect(wgsl).toContain(emitted);
            expect(wgsl).toMatch(/let v\d+: vec3f = f\d+\(/);
        });

        it('reads the channels of a color modulator, and a field for every channel', () => {
            const color = wgslOf(
                `$v.out($v.modulateScale($v.hsv($v.ramp()), $v.hsv(${noise})));`,
            );
            expect(color).toContain('.r;');
            expect(color).toContain('.g;');
            const field = wgslOf(
                `$v.out($v.modulateScale($v.hsv($v.ramp()), ${noise}));`,
            );
            expect(field).not.toContain('.r;');
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

        it('reads its angles and its scroll as fractions of 5, and its counts as they are', () => {
            const rotate = wgslOf(
                `$v.out($v.modulateRotate($v.hsv($v.ramp()), ${noise}, 2.5, 5));`,
            );
            expect(rotate).toMatch(
                /video_transform\(uv, 1\.0, 1\.0 \+ v\d+ \* 0\.5/,
            );
            const cells = wgslOf(
                `$v.out($v.modulatePixelate($v.hsv($v.ramp()), ${noise}, 12, 4));`,
            );
            expect(cells).toMatch(/vec2f\(4\.0 \+ v\d+ \* 12\.0/);
        });

        it('rejects a modulator that is not a video signal', () => {
            for (const [name] of names) {
                expect(() => exec(`$v.${name}($v.hsv(0), 'noise');`)).toThrow(
                    new RegExp(
                        `\\$v\\.${name}: modulator must be a video field or color`,
                    ),
                );
            }
        });
    });
});
