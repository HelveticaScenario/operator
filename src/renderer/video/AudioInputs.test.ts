import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AudioInputs } from './AudioInputs';

const device = {
    createTexture: () => ({ createView: () => ({}), destroy() {} }),
    queue: { writeTexture() {} },
} as unknown as GPUDevice;

const history = (tap: number) => ({ samples: 4, tap, trigger: false });
const chunk = (tap: number) => ({
    sampleRate: 48_000,
    samples: Float32Array.from([1, 2, 3, 4]),
    tap,
});

describe('AudioInputs.reset', () => {
    beforeEach(() => {
        vi.stubGlobal('GPUTextureUsage', { COPY_DST: 2, TEXTURE_BINDING: 4 });
    });

    it('keeps the samples of a slot whose stamp is unchanged', () => {
        const inputs = new AudioInputs(device);
        inputs.reset([history(0), history(1)], [7, 8]);
        inputs.push([chunk(0), chunk(1)]);
        const forgotten = inputs.reset([history(0), history(1)], [7, 9]);
        expect(forgotten).toEqual([1]);
        expect(
            inputs.ready({ histories: [history(0)], tapSlots: [] } as never),
        ).toBe(true);
        expect(
            inputs.ready({ histories: [history(1)], tapSlots: [] } as never),
        ).toBe(false);
    });

    it('lists the slots a shader newly uses as needing a fresh read', () => {
        const inputs = new AudioInputs(device);
        inputs.reset([history(0)], [7]);
        inputs.push([chunk(0)]);
        expect(inputs.reset([history(0), history(2)], [7, 0, 4])).toEqual([2]);
    });

    it('forgets everything for a shader without stamps', () => {
        const inputs = new AudioInputs(device);
        inputs.reset([history(0)], [7]);
        inputs.push([chunk(0)]);
        expect(inputs.reset([history(0)])).toEqual([0]);
    });

    it('forgets everything when cleared', () => {
        const inputs = new AudioInputs(device);
        inputs.reset([history(0)], [7]);
        inputs.push([chunk(0)]);
        inputs.clear();
        expect(
            inputs.ready({ histories: [history(0)], tapSlots: [] } as never),
        ).toBe(false);
    });
});
