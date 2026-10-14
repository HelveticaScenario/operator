import { describe, expect, test } from 'vitest';
import {
    MAX_VIDEO_TAPS as ENGINE_MAX_VIDEO_TAPS,
    VIDEO_HISTORY_LEN as ENGINE_VIDEO_HISTORY_LEN,
} from '@modular/core';
import { MAX_VIDEO_TAPS, VIDEO_HISTORY_LEN } from './videoGraph';

// The renderer bundles this module, so it cannot load the native engine and
// mirrors the engine's limits instead.
describe('video tap limits', () => {
    test("match the engine's", () => {
        expect(MAX_VIDEO_TAPS).toBe(ENGINE_MAX_VIDEO_TAPS);
        expect(VIDEO_HISTORY_LEN).toBe(ENGINE_VIDEO_HISTORY_LEN);
    });
});
