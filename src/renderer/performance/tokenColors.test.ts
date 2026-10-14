import { describe, expect, it } from 'vitest';
import { mapVSCodeTheme, type VSCodeTheme } from '../themes/types';
import { TokenColors } from './tokenColors';

const theme = (
    tokenColors: VSCodeTheme['tokenColors'],
    colors: Record<string, string> = {},
) =>
    mapVSCodeTheme({
        colors: { 'editor.foreground': '#cccccc', ...colors },
        name: 'Test',
        tokenColors,
        type: 'dark',
    });

describe('TokenColors', () => {
    it('colors a token by the theme rule for its type', () => {
        const colors = new TokenColors(
            theme([
                {
                    scope: 'string',
                    settings: { fontStyle: 'bold', foreground: '#00ff00' },
                },
            ]),
        );
        expect(colors.style('string.js')).toEqual({
            bold: true,
            color: '#00ff00',
            italic: false,
        });
    });

    it('falls back from a dotted type to its parent', () => {
        const colors = new TokenColors(
            theme([{ scope: 'comment', settings: { foreground: '#777777' } }]),
        );
        expect(colors.style('comment.doc.js').color).toBe('#777777');
    });

    it("takes the base theme's color for a type the theme leaves out", () => {
        const colors = new TokenColors(theme([]));
        expect(colors.style('number.js').color).toBe('#b5cea8');
        expect(colors.style('identifier.js').color).toBe('#cccccc');
    });

    it('ignores a dotted TextMate scope, as the editor does', () => {
        const colors = new TokenColors(
            theme([
                {
                    scope: 'entity.name.function',
                    settings: { foreground: '#61afef' },
                },
            ]),
        );
        expect(colors.style('identifier.js').color).toBe('#cccccc');
    });

    it('cycles bracket colors by nesting level, skipping transparent ones', () => {
        const colors = new TokenColors(
            theme([], {
                'editorBracketHighlight.foreground1': '#111111',
                'editorBracketHighlight.foreground2': '#22222200',
                'editorBracketHighlight.foreground3': '#333333',
            }),
        );
        expect([0, 1, 2].map((level) => colors.bracket(level))).toEqual([
            '#111111',
            '#333333',
            '#111111',
        ]);
    });

    it("takes Monaco's bracket colors where the theme sets none", () => {
        const colors = new TokenColors(theme([]));
        expect(colors.bracket(0)).toBe('#FFD700');
        expect(colors.bracket(3)).toBe('#FFD700');
        expect(colors.bracket(null)).toBe('rgba(255, 18, 18, 0.8)');
    });
});
