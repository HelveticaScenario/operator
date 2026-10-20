import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';

describe('$v in the DSL executor', () => {
    it('returns no video shader when the patch never calls $v.out', () => {
        expect(exec(`$v.ramp();`).video).toBeNull();
    });

    it('compiles the graph reachable from $v.out', () => {
        const { video } = exec(`
            const x = $v.ramp('horizontal');
            const wave = $v.osc(4, $v.time, { shape: 'triangle', input: x });
            $v.ramp('vertical');
            $v.out($v.solid(wave, 0.5, 1));
        `);
        expect(video).not.toBeNull();
        expect(video!.wgsl).toContain('abs(2.0 * fract(');
        expect(video!.wgsl).not.toContain('uv.y');
    });

    it('draws a generator at the position of each pixel unless told otherwise', () => {
        const implicit = exec(`$v.osc(4).out();`).video!.wgsl;
        const explicit = exec(
            `$v.osc(4, 0, { input: $v.ramp('horizontal') }).out();`,
        ).video!.wgsl;
        expect(implicit).toBe(explicit);
        expect(implicit).toContain('vec4f(vec3f(uv.x), 1.0)');
    });

    it('rejects an output that is not a video signal', () => {
        expect(() => exec(`$v.out('red');`)).toThrow(
            /\$v\.out: input must be a number, a video signal or an audio signal/,
        );
    });

    it('rejects a non-numeric argument', () => {
        expect(() => exec(`$v.osc('fast');`)).toThrow(
            /\$v\.osc: freq must be a number, a video signal or an audio signal/,
        );
    });

    it('names the spelled-out parameter of hsv that failed', () => {
        expect(() => exec(`$v.hsv(0, 'full');`)).toThrow(
            /\$v\.hsv: saturation must be a number, a video signal or an audio signal/,
        );
    });

    it('reports an invalid enumerated option', () => {
        expect(() => exec(`$v.out($v.solid($v.ramp('z'), 0, 0));`)).toThrow(
            /param "axis" must be one of/,
        );
    });

    it('gives every documented member of $v a value', () => {
        let missing = '';
        try {
            exec(
                `throw new Error('missing:' + Object.keys($v).filter((k) => $v[k] === undefined).join(','));`,
            );
        } catch (error) {
            missing = (error as Error).message.split('missing:')[1];
        }
        expect(missing).toBe('');
    });

    describe('output', () => {
        it('has an output when the patch calls $v.out', () => {
            expect(exec(`$v.ramp().out();`).video!.hasOutput).toBe(true);
        });

        it('has no output when a patch only previews', () => {
            const { video } = exec(`$v.ramp().preview();`);
            expect(video).not.toBeNull();
            expect(video!.hasOutput).toBe(false);
        });
    });

    describe('range', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('maps each channel from 0 to 1 onto min to max', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.ramp('angular', { zoom: $v.time.$.osc(1).range(1, 3) })));
            `);
            expect(wgsl).toContain('return vec4f(lo + x * (hi - lo), c.a);');
            expect(wgsl).toMatch(/video_range\(v\d+, 1\.0, 3\.0\)/);
        });

        it('has range as a method of a signal and as $v.range', () => {
            const method = wgslOf(`$v.out($v.hsv($v.ramp().range(0, 0.5)));`);
            const call = wgslOf(`$v.out($v.hsv($v.range($v.ramp(), 0, 0.5)));`);
            expect(method).toBe(call);
        });

        it('rescales a color channel by channel', () => {
            expect(wgslOf(`$v.hsv(0.3).range(0.2, 0.8).out();`)).toMatch(
                /video_range\(v\d+, 0\.2, 0\.8\)/,
            );
        });
    });
});
