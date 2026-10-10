import { describe, expect, it } from 'vitest';
import { syphonAction } from '../syphonTarget';

describe('syphonAction', () => {
    it('starts when nothing is published, whichever window is asked for', () => {
        expect(syphonAction(false, 'editor', 'editor')).toBe('start');
        expect(syphonAction(false, 'editor', 'performance')).toBe('start');
    });

    it('stops when the window asked for is the one published', () => {
        expect(syphonAction(true, 'editor', 'editor')).toBe('stop');
        expect(syphonAction(true, 'performance', 'performance')).toBe('stop');
    });

    it('moves publishing over when the other window is asked for', () => {
        expect(syphonAction(true, 'editor', 'performance')).toBe('switch');
        expect(syphonAction(true, 'performance', 'editor')).toBe('switch');
    });
});
