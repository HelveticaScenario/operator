import { describe, expect, it, vi } from 'vitest';

vi.mock('electron', () => ({ app: {}, dialog: {} }));

const { cpuinfoOutcome, interpretProbe } = await import('../cpuGate');

const exit = (status: number | null, signal: NodeJS.Signals | null = null) => ({
    error: undefined,
    signal,
    status,
});

describe('interpretProbe', () => {
    it('passes a clean exit', () => {
        expect(interpretProbe(exit(0))).toBe('supported');
    });

    it('blocks a reported missing extension or an illegal-instruction crash', () => {
        expect(interpretProbe(exit(3))).toBe('unsupported');
        expect(interpretProbe(exit(null, 'SIGILL'))).toBe('unsupported');
        expect(interpretProbe(exit(0xc000001d))).toBe('unsupported');
    });

    it('lets startup continue when the probe fails for any other reason', () => {
        expect(interpretProbe(exit(4))).toBe('inconclusive');
        expect(interpretProbe(exit(1))).toBe('inconclusive');
        expect(interpretProbe(exit(null, 'SIGSEGV'))).toBe('inconclusive');
        expect(
            interpretProbe({
                error: new Error('spawnSync ETIMEDOUT'),
                signal: 'SIGTERM',
                status: null,
            }),
        ).toBe('inconclusive');
    });
});

const cpuinfo = (flags: string) =>
    `processor\t: 0\nvendor_id\t: GenuineIntel\nflags\t\t: ${flags}\nbugs\t\t:\n\nprocessor\t: 1\nflags\t\t: ${flags}\n`;

describe('cpuinfoOutcome', () => {
    it('passes a CPU with every x86-64-v2 flag', () => {
        expect(
            cpuinfoOutcome(
                cpuinfo(
                    'fpu sse sse2 pni ssse3 cx16 sse4_1 sse4_2 popcnt lahf_lm avx avx2',
                ),
            ),
        ).toBe('supported');
    });

    it('blocks a CPU missing one, such as a K10 without SSE4.1', () => {
        expect(
            cpuinfoOutcome(
                cpuinfo('fpu sse sse2 pni cx16 popcnt lahf_lm sse4a abm'),
            ),
        ).toBe('unsupported');
    });

    it('is inconclusive without a flags line', () => {
        expect(cpuinfoOutcome('processor\t: 0\n')).toBe('inconclusive');
    });
});
