import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';
import { callWgsl } from './wgslEval';

const wgslOf = (source: string) => exec(source).video!.wgsl;

describe('$v alpha and compositing', () => {
    it('lays the output over black by its alpha', () => {
        const wgsl = wgslOf(`$v.solid(1, 0, 0, 0.5).out();`);
        expect(wgsl).toContain('vec4f(1.0, 0.0, 0.0, 0.5)');
        expect(wgsl).toContain('return vec4f(clipped.rgb * clipped.a, 1.0);');
    });

    it('gives solid an opaque alpha by default', () => {
        expect(wgslOf(`$v.solid(1, 0, 0).out();`)).toContain(
            'vec4f(1.0, 0.0, 0.0, 1.0)',
        );
    });

    it('keeps the alpha of a picture through color adjustments', () => {
        const wgsl = wgslOf(`$v.image('a.png').$.hue(0.2).out();`);
        expect(wgsl).toContain(
            'return vec4f(hsv_to_rgb(hsv.x + amount, hsv.y, hsv.z), c.a);',
        );
        expect(wgsl).toContain(
            'textureSampleLevel(media, fb_sampler, vec2f(p.x, 1.0 - p.y), 0.0)',
        );
    });

    it('lays one signal over another by the alpha of the top one', () => {
        const chained = wgslOf(`$v.hsv(0.6).$.layer($v.image('a.png')).out();`);
        const called = wgslOf(
            `$v.out($v.layer($v.hsv(0.6), $v.image('a.png')));`,
        );
        expect(chained).toBe(called);
        expect(called).toContain(
            'return vec4f(mix(a.rgb, b.rgb, clamp(b.a, 0.0, 1.0)), clamp(a.a + b.a, 0.0, 1.0));',
        );
    });

    it('masks a signal by the brightness of another, color and alpha alike', () => {
        const wgsl = wgslOf(`$v.osc(8).$.hsv().$.mask($v.shape(0.3)).out();`);
        expect(wgsl).toContain('let k = clamp(m, 0.0, 1.0);');
        expect(wgsl).toContain('return vec4f(c.rgb * k, c.a * k);');
        expect(wgsl).toMatch(/video_mask\(v\d+, video_luma\(v\d+\)\)/);

        const masked = (m: number) =>
            callWgsl(wgsl, 'video_mask', [0.8, 0.4, 0.2, 1], m);
        expect(masked(1)).toEqual([0.8, 0.4, 0.2, 1]);
        expect(masked(0)).toEqual([0, 0, 0, 0]);
        expect(masked(0.5)).toEqual([0.4, 0.2, 0.1, 0.5]);
        expect(callWgsl(wgsl, 'video_mask', [1, 1, 1, 0.5], 2)).toEqual([
            1, 1, 1, 0.5,
        ]);
    });

    it('keys a signal by its own brightness, with an edge centered on the threshold', () => {
        const wgsl = wgslOf(`$v.noise().$.luma(0.4, 0.05).out();`);
        expect(wgsl).toMatch(/video_luma_key\(v\d+, 0\.4, 0\.05\)/);
        expect(wgsl).toContain(
            'let k = smoothstep(threshold - band, threshold + band, l);',
        );

        const keyed = (level: number) =>
            callWgsl(
                wgsl,
                'video_luma_key',
                [level, level, level, 1],
                0.4,
                0.05,
            );
        const near = (actual: unknown, expected: number[]) =>
            (actual as number[]).forEach((v, k) =>
                expect(v).toBeCloseTo(expected[k], 5),
            );
        near(keyed(0.8), [0.8, 0.8, 0.8, 1]);
        near(keyed(0.45), [0.45, 0.45, 0.45, 1]);
        near(keyed(0.35), [0, 0, 0, 0]);
        near(keyed(0.4), [0.2, 0.2, 0.2, 0.5]);
    });

    it('keeps the dark side with invert, mirroring the edge', () => {
        const wgsl = wgslOf(
            `$v.image('a.png').$.luma(0.2, 0.1, { invert: true }).out();`,
        );
        expect(wgsl).toMatch(/video_luma_key_below\(v\d+, 0\.2, 0\.1\)/);
        expect(wgsl).toContain(
            'let k = (1.0 - smoothstep(threshold - band, threshold + band, l));',
        );
        expect(wgsl).not.toContain('fn video_luma_key(');

        const keyed = (level: number, alpha = 1) =>
            callWgsl(
                wgsl,
                'video_luma_key_below',
                [level, level, level, alpha],
                0.2,
                0.1,
            );
        const [r, , , a] = keyed(0.05, 0.5) as number[];
        expect(r).toBeCloseTo(0.05);
        expect(a).toBeCloseTo(0.5);
        expect((keyed(0.5) as number[])[3]).toBeCloseTo(0);
        expect((keyed(0.2) as number[])[3]).toBeCloseTo(0.5);
    });

    it('adds nothing of what a key or mask removes', () => {
        const wgsl = wgslOf(
            `$v.hsv(0.6).$.add($v.osc(8).$.luma(0.5, 0.1)).$.layer($v.hsv(0.1).$.mask($v.shape(0.2))).out();`,
        );
        const base = [0.2, 0.3, 0.9, 1];
        const keyedOut = callWgsl(
            wgsl,
            'video_luma_key',
            [0.3, 0.3, 0.3, 1],
            0.5,
            0.1,
        );
        expect(callWgsl(wgsl, 'video_add', base, keyedOut)).toEqual(base);
        const maskedOut = callWgsl(wgsl, 'video_mask', [1, 0.5, 0, 1], 0);
        expect(callWgsl(wgsl, 'video_layer', base, maskedOut)).toEqual(base);
        expect(
            callWgsl(wgsl, 'video_layer', keyedOut, [1, 0.5, 0, 0.5]),
        ).toEqual([0.5, 0.25, 0, 0.5]);
    });

    it('scales only the alpha of a mask with keepColor', () => {
        const chained = wgslOf(
            `$v.osc(8).$.hsv().$.mask($v.shape(0.3), { keepColor: true }).out();`,
        );
        const called = wgslOf(
            `$v.out($v.mask($v.osc(8).$.hsv(), $v.shape(0.3), { keepColor: true }));`,
        );
        expect(chained).toBe(called);
        expect(called).toMatch(
            /video_mask_keep_color\(v\d+, video_luma\(v\d+\)\)/,
        );
        expect(called).not.toContain('fn video_mask(');

        const masked = (m: number) =>
            callWgsl(called, 'video_mask_keep_color', [0.8, 0.4, 0.2, 1], m);
        expect(masked(0)).toEqual([0.8, 0.4, 0.2, 0]);
        expect(masked(0.5)).toEqual([0.8, 0.4, 0.2, 0.5]);
    });

    it('scales only the alpha of a luma key with keepColor, on either side', () => {
        const above = wgslOf(
            `$v.noise().$.luma(0.4, 0.05, { keepColor: true }).out();`,
        );
        expect(above).toMatch(/video_luma_key_keep_color\(v\d+, 0\.4, 0\.05\)/);
        expect(
            callWgsl(
                above,
                'video_luma_key_keep_color',
                [0.3, 0.3, 0.3, 1],
                0.4,
                0.05,
            ),
        ).toEqual([0.3, 0.3, 0.3, 0]);

        const below = wgslOf(
            `$v.noise().$.luma(0.2, 0.1, { invert: true, keepColor: true }).out();`,
        );
        expect(
            callWgsl(
                below,
                'video_luma_key_below_keep_color',
                [0.5, 0.5, 0.5, 1],
                0.2,
                0.1,
            ),
        ).toEqual([0.5, 0.5, 0.5, 0]);
    });

    it('reads the masked-out color of a keepColor mask as a number', () => {
        const wgsl = wgslOf(
            `$v.hsv(0.6).$.hue($v.osc(8).$.mask(0, { keepColor: true })).out();`,
        );
        expect(wgsl).toMatch(
            /let (v\d+): vec4f = video_mask_keep_color\(v\d+, 0\.0\);\s+let v\d+: vec4f = video_hue\(v\d+, video_luma\(\1\)\);/,
        );
        const maskedOut = callWgsl(
            wgsl,
            'video_mask_keep_color',
            [0.8, 0.8, 0.8, 1],
            0,
        );
        expect(callWgsl(wgsl, 'video_luma', maskedOut)).toBeCloseTo(0.8);
    });

    it('rejects a keepColor that is not true or false', () => {
        expect(() => exec(`$v.osc().$.mask(0.5, { keepColor: 1 });`)).toThrow(
            /\$v\.mask: keepColor must be true or false/,
        );
        expect(() =>
            exec(`$v.noise().$.luma(0.5, 0.1, { keepColor: 'yes' });`),
        ).toThrow(/\$v\.luma: keepColor must be true or false/);
    });

    it('rejects an invert that is not true or false', () => {
        expect(() =>
            exec(`$v.noise().$.luma(0.5, 0.1, { invert: 'yes' });`),
        ).toThrow(/\$v\.luma: invert must be true or false/);
    });

    it('stores the alpha in a frame buffer', () => {
        const wgsl = wgslOf(`
            $v.out($v.feedback((prev) => $v.layer(prev, $v.solid(1, 0, 0, 0.1))));
        `);
        expect(wgsl).toMatch(
            /return FragOut\(video_display\(v\d+\), clamp\(v\d+, vec4f\(0\.0\), vec4f\(1\.0\)\)\);/,
        );
    });
});
