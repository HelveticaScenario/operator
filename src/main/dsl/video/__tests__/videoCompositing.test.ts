import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';

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

    it('masks a signal by the brightness of another, keeping its color', () => {
        const wgsl = wgslOf(`$v.osc(8).$.hsv().$.mask($v.shape(0.3)).out();`);
        expect(wgsl).toContain(
            'return vec4f(c.rgb, c.a * clamp(m, 0.0, 1.0));',
        );
        expect(wgsl).toMatch(/video_mask\(v\d+, video_luma\(v\d+\)\)/);
    });

    it('keys a signal by its own brightness, opaque from the threshold up', () => {
        const wgsl = wgslOf(`$v.noise().$.luma(0.4, 0.05).out();`);
        expect(wgsl).toMatch(/video_luma_key\(v\d+, 0\.4, 0\.05\)/);
        expect(wgsl).toContain(
            'smoothstep(threshold - max(tolerance, 0.00001), threshold, video_luma(c))',
        );
        expect(wgsl).toContain('return vec4f(c.rgb, c.a * a);');
    });

    it('stores the alpha in a frame buffer', () => {
        const wgsl = wgslOf(`
            $v.out($v.feedback((prev) => $v.layer(prev, $v.colorize(1, 0, 0, 0.1))));
        `);
        expect(wgsl).toMatch(/return FragOut\(video_display\(v\d+\), v\d+\);/);
    });
});
