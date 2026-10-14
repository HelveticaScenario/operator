import { Color } from 'monaco-editor/esm/vs/base/common/color.js';
import {
    editorBracketHighlightingForeground1,
    editorBracketHighlightingForeground2,
    editorBracketHighlightingForeground3,
    editorBracketHighlightingForeground4,
    editorBracketHighlightingForeground5,
    editorBracketHighlightingForeground6,
    editorBracketHighlightingUnexpectedBracketForeground,
} from 'monaco-editor/esm/vs/editor/common/core/editorColorRegistry.js';
import { TokenMetadata } from 'monaco-editor/esm/vs/editor/common/encodedTokenAttributes.js';
import {
    TokenTheme,
    type ITokenThemeRule,
} from 'monaco-editor/esm/vs/editor/common/languages/supports/tokenization.js';
import * as builtinThemes from 'monaco-editor/esm/vs/editor/standalone/common/themes.js';
import {
    Registry,
    type ColorTheme,
} from 'monaco-editor/esm/vs/platform/registry/common/platform.js';
import { Extensions } from 'monaco-editor/esm/vs/platform/theme/common/colorUtils.js';
import { monacoThemeData } from '../components/monaco/theme';
import type { AppTheme } from '../themes/types';

/** How a token is drawn: its color and whether it is bold or italic. */
export interface TokenStyle {
    color: string;
    bold: boolean;
    italic: boolean;
}

/** Monaco's FontStyle bits. */
const ITALIC = 1;
const BOLD = 2;

const BRACKET_LEVEL_COLORS = [
    editorBracketHighlightingForeground1,
    editorBracketHighlightingForeground2,
    editorBracketHighlightingForeground3,
    editorBracketHighlightingForeground4,
    editorBracketHighlightingForeground5,
    editorBracketHighlightingForeground6,
];

/**
 * Resolves Monaco token types (`keyword.js`, `identifier.js`, ...) and
 * bracket pair levels to the colors the editor gives them in a theme. The
 * token theme and colors are built as Monaco's StandaloneTheme builds them,
 * from the same theme data as the editor.
 */
export class TokenColors {
    private readonly cache = new Map<string, TokenStyle>();
    private readonly tokenTheme: TokenTheme;
    private readonly colorMap: string[];
    private readonly brackets: string[];
    private readonly unexpectedBracket: string | undefined;

    constructor(theme: AppTheme) {
        const data = monacoThemeData(theme);
        const base =
            data.base === 'vs' ? builtinThemes.vs : builtinThemes.vs_dark;
        const rules: ITokenThemeRule[] = [...base.rules];
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
        this.colorMap = this.tokenTheme
            .getColorMap()
            .map((color) => color?.toString() ?? '');

        const registry = Registry.as(Extensions.ColorContribution);
        const colorTheme: ColorTheme = {
            getColor: (id) => {
                const hex = data.colors[id];
                return hex
                    ? Color.fromHex(hex)
                    : registry.resolveDefaultColor(id, colorTheme);
            },
            type: theme.type,
        };
        this.brackets = BRACKET_LEVEL_COLORS.flatMap((id) => {
            const color = colorTheme.getColor(id);
            return color === undefined || color.isTransparent()
                ? []
                : [color.toString()];
        });
        this.unexpectedBracket = colorTheme
            .getColor(editorBracketHighlightingUnexpectedBracketForeground)
            ?.toString();
    }

    /** The style of a token of `type`. */
    style(type: string): TokenStyle {
        let style = this.cache.get(type);
        if (style === undefined) {
            const metadata = this.tokenTheme.match(0, type);
            const fontStyle = TokenMetadata.getFontStyle(metadata);
            style = {
                bold: (fontStyle & BOLD) !== 0,
                color: this.colorMap[TokenMetadata.getForeground(metadata)],
                italic: (fontStyle & ITALIC) !== 0,
            };
            this.cache.set(type, style);
        }
        return style;
    }

    /**
     * The color of a bracket at nesting `level`, or of an unmatched closing
     * bracket for null; undefined when the theme gives it none.
     */
    bracket(level: number | null): string | undefined {
        if (level === null) return this.unexpectedBracket;
        return this.brackets.length === 0
            ? undefined
            : this.brackets[level % this.brackets.length];
    }
}
