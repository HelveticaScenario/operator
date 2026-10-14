import { describe, expect, it } from 'vitest';
import { mapVSCodeTheme, type VSCodeTheme } from '../themes/types';
import { TokenColors } from './tokenColors';

const theme = (tokenColors: VSCodeTheme['tokenColors']) =>
    mapVSCodeTheme({
        colors: { 'editor.foreground': '#cccccc' },
        name: 'Test',
        tokenColors,
        type: 'dark',
    });

describe('TokenColors', () => {
    it('colors a token by the theme rule for its scope', () => {
        const colors = new TokenColors(
            theme([{ scope: 'string', settings: { foreground: '#00ff00' } }]),
        );
        expect(colors.style('string.js', "'c4'").color).toBe('#00ff00');
    });

    it('prefers the most specific rule that covers the scope', () => {
        const colors = new TokenColors(
            theme([
                { scope: 'constant', settings: { foreground: '#111111' } },
                {
                    scope: ['constant.numeric'],
                    settings: { fontStyle: 'bold', foreground: '#222222' },
                },
            ]),
        );
        expect(colors.style('number.js', '440')).toEqual({
            bold: true,
            color: '#222222',
            italic: false,
        });
    });

    it('ignores a rule more specific than the scope', () => {
        const colors = new TokenColors(
            theme([
                {
                    scope: 'keyword.control',
                    settings: { foreground: '#ff0000' },
                },
            ]),
        );
        expect(colors.style('keyword.js', 'const').color).toBe('#ff0000');
        const plain = new TokenColors(
            theme([
                { scope: 'string.quoted', settings: { foreground: '#ff0000' } },
            ]),
        );
        expect(plain.style('string.js', "'a'").color).toBe('#cccccc');
    });

    it('colors a $ identifier as a function call', () => {
        const colors = new TokenColors(
            theme([
                {
                    scope: 'entity.name.function',
                    settings: { foreground: '#61afef' },
                },
            ]),
        );
        expect(colors.style('identifier.js', '$sine').color).toBe('#61afef');
        expect(colors.style('identifier.js', 'osc').color).toBe('#cccccc');
    });

    it('falls back from a dotted type to its parent', () => {
        const colors = new TokenColors(
            theme([{ scope: 'comment', settings: { foreground: '#777777' } }]),
        );
        expect(colors.style('comment.doc.js', '/** x */').color).toBe(
            '#777777',
        );
    });
});
