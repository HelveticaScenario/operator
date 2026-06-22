/**
 * Tests for tinykeys binding -> Electron accelerator formatting.
 */
import { describe, expect, test } from 'vitest';
import { toElectronAccelerator } from './accelerator';

describe('toElectronAccelerator', () => {
    test('maps modifiers and plain keys', () => {
        expect(toElectronAccelerator('Meta+Enter')).toBe('Cmd+Enter');
        expect(toElectronAccelerator('Control+s')).toBe('Ctrl+S');
        expect(toElectronAccelerator('Shift+Meta+p')).toBe('Shift+Cmd+P');
        expect(toElectronAccelerator('Meta+.')).toBe('Cmd+.');
    });

    test('maps function keys and arrows', () => {
        expect(toElectronAccelerator('F12')).toBe('F12');
        expect(toElectronAccelerator('Shift+F12')).toBe('Shift+F12');
        expect(toElectronAccelerator('Alt+F12')).toBe('Alt+F12');
        expect(toElectronAccelerator('Meta+ArrowRight')).toBe('Cmd+Right');
    });

    test('decodes the (code) regex form back to a key', () => {
        expect(toElectronAccelerator('Alt+Shift+(KeyI)')).toBe('Alt+Shift+I');
        expect(toElectronAccelerator('Shift+Meta+(Digit0)')).toBe(
            'Shift+Cmd+0',
        );
        expect(toElectronAccelerator('Alt+(BracketLeft)')).toBe('Alt+[');
    });

    test('orders modifiers canonically', () => {
        expect(toElectronAccelerator('Shift+Alt+f')).toBe('Alt+Shift+F');
    });

    test('returns null for chord sequences (no Electron equivalent)', () => {
        expect(toElectronAccelerator('Meta+k Meta+i')).toBeNull();
    });

    test('returns null for empty input', () => {
        expect(toElectronAccelerator('')).toBeNull();
    });
});
