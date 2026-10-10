import { describe, expect, it } from 'vitest';
import { gateVerdict, ShaderGate } from './shaderGate';

describe('gateVerdict', () => {
    it('waits while the engine is still on an earlier patch', () => {
        expect(gateVerdict(5, 4, 0)).toBe('wait');
    });

    it('activates when the engine applies its update', () => {
        expect(gateVerdict(5, 5, 0)).toBe('activate');
    });

    it('activates when the engine has gone past its update', () => {
        expect(gateVerdict(5, 7, 0)).toBe('activate');
    });

    it('drops a shader whose update the engine discarded', () => {
        expect(gateVerdict(5, 4, 5)).toBe('drop');
    });

    it('prefers a later applied update to an earlier discard', () => {
        expect(gateVerdict(5, 6, 5)).toBe('activate');
    });
});

describe('ShaderGate', () => {
    const program = () => ({
        destroyed: false,
        destroy() {
            this.destroyed = true;
        },
    });

    it('releases a shader once the engine applies its update', () => {
        const gate = new ShaderGate<ReturnType<typeof program>>();
        const built = program();
        gate.offer(gate.request(3), built);
        gate.report(2, 0);
        expect(gate.settle(false)).toBeUndefined();
        expect(gate.holding).toBe(false);
        gate.report(3, 0);
        expect(gate.holding).toBe(true);
        expect(gate.settle(false)).toEqual({ program: built });
        expect(gate.swapping).toBe(false);
    });

    it('drops a build that a newer request superseded', () => {
        const gate = new ShaderGate<ReturnType<typeof program>>();
        const stale = program();
        const token = gate.request(1);
        gate.request(2);
        expect(gate.offer(token, stale)).toBe(false);
        expect(stale.destroyed).toBe(true);
    });

    it('destroys a shader whose update the engine discarded', () => {
        const gate = new ShaderGate<ReturnType<typeof program>>();
        const built = program();
        gate.offer(gate.request(4), built);
        gate.report(3, 4);
        expect(gate.settle(false)).toBeUndefined();
        expect(built.destroyed).toBe(true);
        expect(gate.swapping).toBe(false);
    });

    it('releases regardless when forced after the engine restarts its numbering', () => {
        const gate = new ShaderGate<ReturnType<typeof program>>();
        gate.report(9, 0);
        const built = program();
        gate.offer(gate.request(10), built);
        expect(gate.report(1, 0)).toBe(true);
        expect(gate.settle(true)).toEqual({ program: built });
    });

    it('stops waiting when the build fails', () => {
        const gate = new ShaderGate<ReturnType<typeof program>>();
        gate.fail(gate.request(5));
        expect(gate.swapping).toBe(false);
    });
});
