import { describe, expect, it, vi } from 'vitest';

vi.mock('electron', () => ({ app: {}, dialog: {} }));

const { interpretProbe } = await import('../cpuGate');

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
