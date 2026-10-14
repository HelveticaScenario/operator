import { describe, expect, it } from 'vitest';
import { withinTextureLimit } from './canvasSize';

describe('withinTextureLimit', () => {
    it('leaves a size inside the limit as it is', () => {
        expect(withinTextureLimit(1920, 1080, 8192)).toEqual({
            height: 1080,
            width: 1920,
        });
    });

    it('scales a size past the limit down to it, keeping the aspect ratio', () => {
        expect(withinTextureLimit(16_000, 4000, 8192)).toEqual({
            height: 2048,
            width: 8192,
        });
    });

    it('limits the taller side of a portrait size', () => {
        expect(withinTextureLimit(3000, 12_000, 8192)).toEqual({
            height: 8192,
            width: 2048,
        });
    });

    it('never shrinks a side below one pixel', () => {
        expect(withinTextureLimit(100_000, 1, 8192).height).toBe(1);
    });
});
