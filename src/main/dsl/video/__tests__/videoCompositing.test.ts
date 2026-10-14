import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';
import { callWgsl } from './wgslEval';

const wgslOf = (source: string) => exec(source).video!.wgsl;

describe('$v alpha and compositing', () => {
    it('lays the output over black by its alpha', () => {
        const wgsl = wgslOf(`$v.colorize(1, 0, 0, 0.5).out();`);
        expect(wgsl).toContain('clamp(vec4f(1.0, 0.0, 0.0, 0.5)');
        expect(wgsl).toContain('return vec4f(clipped.rgb * clipped.a, 1.0);');
    });

    it('gives colorize an opaque alpha by default', () => {
        expect(wgslOf(`$v.colorize(1, 0, 0).out();`)).toContain(
            'vec4f(1.0, 0.0, 0.0, 1.0)',
        );
    });

    it('keeps the alpha of a picture through color adjustments', () => {
        const wgsl = wgslOf(`$v.image('a.png').$.hueShift(0.2).out();`);
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

    it('keys a signal by its own brightness, opaque from the threshold up', () => {
        const wgsl = wgslOf(`$v.noise().$.luma(0.4, 0.05).out();`);
        expect(wgsl).toMatch(/video_luma_key\(v\d+, 0\.4, 0\.05\)/);
        expect(wgsl).toContain(
            'let k = smoothstep(threshold - band, threshold, l);',
        );

        const keyed = (level: number) =>
            callWgsl(
                wgsl,
                'video_luma_key',
                [level, level, level, 1],
                0.4,
                0.05,
            );
        expect(keyed(0.8)).toEqual([0.8, 0.8, 0.8, 1]);
        expect(keyed(0.4)).toEqual([0.4, 0.4, 0.4, 1]);
        expect(keyed(0.3)).toEqual([0, 0, 0, 0]);
        const [r, , , a] = keyed(0.375) as number[];
        expect(a).toBeCloseTo(0.5);
        expect(r).toBeCloseTo(0.375 * 0.5);
    });

    it('keeps the dark side with invert, opaque up to the threshold', () => {
        const wgsl = wgslOf(
            `$v.image('a.png').$.luma(0.2, 0.1, { invert: true }).out();`,
        );
        expect(wgsl).toMatch(/video_luma_key_below\(v\d+, 0\.2, 0\.1\)/);
        expect(wgsl).toContain(
            'let k = (1.0 - smoothstep(threshold, threshold + band, l));',
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
        expect(keyed(0.1, 0.5)).toEqual([0.1, 0.1, 0.1, 0.5]);
        expect(keyed(0.5)).toEqual([0, 0, 0, 0]);
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

    it('rejects an invert that is not true or false', () => {
        expect(() =>
            exec(`$v.noise().$.luma(0.5, 0.1, { invert: 'yes' });`),
        ).toThrow(/\$v\.luma: invert must be true or false/);
    });

    it('stores the alpha in a frame buffer', () => {
        const wgsl = wgslOf(`
            $v.out($v.feedback((prev) => $v.layer(prev, $v.colorize(1, 0, 0, 0.1))));
        `);
        expect(wgsl).toMatch(/return FragOut\(video_display\(v\d+\), v\d+\);/);
    });
});
