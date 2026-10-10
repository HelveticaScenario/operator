import { describe, expect, it } from 'vitest';
import { gateVerdict } from './shaderGate';

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
