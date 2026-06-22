import { describe, expect, test } from 'vitest';
import { isBufferSwitch } from '../bufferSwitch';

describe('isBufferSwitch', () => {
    test('re-applying the same buffer is not a switch', () => {
        expect(isBufferSwitch('song-a', 'song-a')).toBe(false);
    });

    test('the very first apply (no previous source) is not a switch', () => {
        expect(isBufferSwitch(null, 'song-a')).toBe(false);
    });

    test('switching to a different buffer is a switch', () => {
        expect(isBufferSwitch('song-a', 'song-b')).toBe(true);
    });

    test('a missing/empty next source id is not a switch', () => {
        expect(isBufferSwitch('song-a', undefined)).toBe(false);
        expect(isBufferSwitch('song-a', '')).toBe(false);
    });
});
