import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';

describe('$v in the DSL executor', () => {
    it('returns no video shader when the patch never calls $v.out', () => {
        expect(exec(`$v.ramp();`).video).toBeNull();
    });

    it('compiles the graph reachable from $v.out', () => {
        const { video } = exec(`
            const x = $v.ramp('h');
            const wave = $v.osc(x, 4, $v.time, { shape: 'triangle' });
            $v.ramp('v');
            $v.out($v.colorize(wave, 0.5, 1));
        `);
        expect(video).not.toBeNull();
        expect(video!.wgsl).toContain('abs(2.0 * fract(');
        expect(video!.wgsl).not.toContain('uv.y');
    });

    it('shows a field as the gray color of its level', () => {
        const field = exec(`$v.out($v.ramp());`).video!.wgsl;
        const gray = exec(`
            const r = $v.ramp();
            $v.out($v.colorize(r, r, r));
        `).video!.wgsl;
        expect(field).toBe(gray);
    });

    it('rejects an output that is not a video signal', () => {
        expect(() => exec(`$v.out('red');`)).toThrow(
            /\$v\.out: input must be a number or a video field/,
        );
    });

    it('rejects a non-numeric field argument', () => {
        expect(() => exec(`$v.osc($v.ramp(), 'fast');`)).toThrow(
            /\$v\.osc: freq must be a number or a video field/,
        );
    });

    it('reports an invalid enumerated option', () => {
        expect(() => exec(`$v.out($v.colorize($v.ramp('z'), 0, 0));`)).toThrow(
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

    describe('volts', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('reads a constant as a fraction of 5 volts, so 5 is full', () => {
            expect(wgslOf(`$v.out($v.colorize(5, 2.5, 0));`)).toContain(
                'clamp(vec3f(1.0, 0.5, 0.0)',
            );
        });

        it('hands a natural input its constant as it is, and a phase a fraction of 5', () => {
            expect(
                wgslOf(`$v.out($v.hsv($v.osc($v.ramp(), 10, 1.25)));`),
            ).toMatch(/fract\(v\d \* 10\.0 \+ 0\.25\)/);
        });

        it('reads an audio signal as a fraction of 5 volts, or as volts for a natural input', () => {
            const full = wgslOf(`$v.out($v.hsv($sine('1hz').range(0, 5)));`);
            expect(full).toMatch(/\(u\.slots\[0\]\[0\] \* 0\.2\)/);
            const natural = wgslOf(
                `$v.ramp('h', { zoom: $sine('1hz').range(1, 3) }).$.hsv().out();`,
            );
            expect(natural).toMatch(
                /video_transform\(uv, u\.slots\[0\]\[0\], 0\.0,/,
            );
            const turn = wgslOf(
                `$v.ramp('h', { rotate: $ramp('0.1hz') }).$.hsv().out();`,
            );
            expect(turn).toMatch(
                /video_transform\(uv, 1\.0, \(u\.slots\[0\]\[0\] \* 0\.2\),/,
            );
        });

        it('reads the clock as volts: seconds for a natural input, a fifth of them otherwise', () => {
            expect(wgslOf(`$v.out($v.hsv($v.time));`)).toContain(
                '(u.time * 0.2)',
            );
            expect(
                wgslOf(`$v.out($v.hsv($v.osc($v.ramp(), $v.time)));`),
            ).toMatch(/\* u\.time \+/);
        });

        it('multiplies a field by 5 where a natural input reads it', () => {
            const wgsl = wgslOf(
                `$v.out($v.hsv($v.osc($v.ramp(), $v.ramp('v'))));`,
            );
            expect(wgsl).toMatch(/fract\(v\d \* \(v\d \* 5\.0\)/);
        });

        it('maps 0 to 5 volts onto a range with range', () => {
            const wgsl = wgslOf(`
                $v.out($v.hsv($v.ramp('a', { rotate: $v.osc($v.time, 1).range(0, 1) })));
            `);
            expect(wgsl).toMatch(
                /\(\(0\.0 \+ v\d \* \(1\.0 - 0\.0\)\) \* 0\.2\)/,
            );
        });

        it('has range as a method of a field and as $v.range', () => {
            const method = wgslOf(`$v.out($v.hsv($v.ramp().range(0, 2.5)));`);
            const call = wgslOf(`$v.out($v.hsv($v.range($v.ramp(), 0, 2.5)));`);
            expect(method).toBe(call);
        });

        it('rejects range on a color or a number', () => {
            expect(() => exec(`$v.hsv(0).range(0, 1);`)).toThrow(
                /\$v\.range: input must be a video field, got a color/,
            );
            expect(() => exec(`$v.range(3, 0, 1);`)).toThrow(
                /\$v\.range: input must be a video field/,
            );
        });

        it('reads audio history as a fraction of 5 volts', () => {
            expect(
                wgslOf(`$v.out($v.hsv($v.fromAudio($sine('110hz'))));`),
            ).toContain('* 0.2)');
        });
    });
});
