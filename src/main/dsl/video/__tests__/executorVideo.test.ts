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
});
