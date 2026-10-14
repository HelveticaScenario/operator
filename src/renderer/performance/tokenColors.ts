// monaco-editor ships types only for its main entry; these deep ESM files are
// typed `any`. They are the token-theme engine the editor itself colors with.
// @ts-expect-error -- no shipped types for this deep ESM path (monaco-editor#4612)
import { TokenTheme } from 'monaco-editor/esm/vs/editor/common/languages/supports/tokenization.js';
// @ts-expect-error -- no shipped types for this deep ESM path (monaco-editor#4612)
import * as builtinThemes from 'monaco-editor/esm/vs/editor/standalone/common/themes.js';
import { monacoThemeData } from '../components/monaco/theme';
import type { AppTheme } from '../themes/types';

/** How a token is drawn: its color and whether it is bold or italic. */
export interface TokenStyle {
    color: string;
    bold: boolean;
    italic: boolean;
}

/** Monaco's encoded token metadata: font style bits and the color map index. */
const FONT_STYLE_MASK = 0x7800;
const FONT_STYLE_OFFSET = 11;
const FOREGROUND_MASK = 0xff8000;
const FOREGROUND_OFFSET = 15;
const ITALIC = 1;
const BOLD = 2;

/** Monaco's default bracket pair colors, by nesting level, where a theme sets none. */
const DEFAULT_BRACKET_COLORS = {
    dark: ['#FFD700', '#DA70D6', '#179FFF'],
    light: ['#0431FA', '#319331', '#7B3814'],
};
const DEFAULT_UNEXPECTED_BRACKET = 'rgba(255, 18, 18, 0.8)';

/** A color with no alpha, or one whose alpha is not zero. */
const isOpaque = (color: string) =>
    !/^#(?:[0-9a-f]{3}0|[0-9a-f]{6}00)$/i.test(color);

/**
 * Resolves Monaco token types (`keyword.js`, `identifier.js`, ...) and
 * bracket pair levels to the colors the editor gives them in a theme.
 */
export class TokenColors {
    private readonly cache = new Map<string, TokenStyle>();
    private readonly tokenTheme: {
        match(languageId: number, token: string): number;
        getColorMap(): ({ toString(): string } | undefined)[];
    };
    private readonly colorMap: string[];
    private readonly brackets: string[];
    private readonly unexpectedBracket: string;

    constructor(theme: AppTheme) {
        // Built as Monaco's StandaloneTheme builds its token theme, so the
        // colors match the editor's.
        const data = monacoThemeData(theme);
        const base =
            data.base === 'vs' ? builtinThemes.vs : builtinThemes.vs_dark;
        const rules = [...base.rules];
        const foreground = data.colors['editor.foreground'];
        const background = data.colors['editor.background'];
        if (foreground || background) {
            rules.push({ background, foreground, token: '' });
        }
        rules.push(...data.rules);
        this.tokenTheme = TokenTheme.createFromRawTokenTheme(
            rules,
            base.encodedTokensColors ?? [],
        );
        // Index 0 is unused; Monaco's Color formats as CSS.
        this.colorMap = this.tokenTheme
            .getColorMap()
            .map((color: { toString(): string } | undefined) =>
                color === undefined ? '' : color.toString(),
            );

        const defaults = DEFAULT_BRACKET_COLORS[theme.type];
        this.brackets = [1, 2, 3, 4, 5, 6]
            .map(
                (n) =>
                    theme.raw.colors?.[
                        `editorBracketHighlight.foreground${n}`
                    ] ?? defaults[n - 1],
            )
            .filter(
                (color): color is string =>
                    color !== undefined && isOpaque(color),
            );
        this.unexpectedBracket =
            theme.raw.colors?.[
                'editorBracketHighlight.unexpectedBracket.foreground'
            ] ?? DEFAULT_UNEXPECTED_BRACKET;
    }

    /** The style of a token of `type`. */
    style(type: string): TokenStyle {
        let style = this.cache.get(type);
        if (style === undefined) {
            const metadata: number = this.tokenTheme.match(0, type);
            const fontStyle =
                (metadata & FONT_STYLE_MASK) >>> FONT_STYLE_OFFSET;
            style = {
                bold: (fontStyle & BOLD) !== 0,
                color: this.colorMap[
                    (metadata & FOREGROUND_MASK) >>> FOREGROUND_OFFSET
                ],
                italic: (fontStyle & ITALIC) !== 0,
            };
            this.cache.set(type, style);
        }
        return style;
    }

    /**
     * The color of a bracket at nesting `level`, or of an unmatched closing
     * bracket for null; undefined when the theme colors no levels.
     */
    bracket(level: number | null): string | undefined {
        if (level === null) return this.unexpectedBracket;
        return this.brackets.length === 0
            ? undefined
            : this.brackets[level % this.brackets.length];
    }
}
