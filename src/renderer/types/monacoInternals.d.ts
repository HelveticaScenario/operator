// Types for the deep monaco-editor ESM files the performance view resolves
// token and bracket colors with. monaco-editor ships types only for its main
// entry; these cover just the members used.

declare module 'monaco-editor/esm/vs/base/common/color.js' {
    export class Color {
        static fromHex(hex: string): Color;
        isTransparent(): boolean;
        /** The color as CSS: hex, or rgba() when it has alpha. */
        toString(): string;
    }
}

declare module 'monaco-editor/esm/vs/editor/common/encodedTokenAttributes.js' {
    /** Decodes the metadata a token theme matches a token type to. */
    export const TokenMetadata: {
        getFontStyle(metadata: number): number;
        getForeground(metadata: number): number;
    };
}

declare module 'monaco-editor/esm/vs/editor/common/languages/supports/tokenization.js' {
    import type { Color } from 'monaco-editor/esm/vs/base/common/color.js';

    export interface ITokenThemeRule {
        token: string;
        foreground?: string;
        background?: string;
        fontStyle?: string;
    }

    export class TokenTheme {
        static createFromRawTokenTheme(
            source: ITokenThemeRule[],
            customTokenColors: string[],
        ): TokenTheme;
        /** Index 0 is unused. */
        getColorMap(): (Color | undefined)[];
        /** The encoded metadata of `token`; see TokenMetadata. */
        match(languageId: number, token: string): number;
    }
}

declare module 'monaco-editor/esm/vs/editor/standalone/common/themes.js' {
    import type { ITokenThemeRule } from 'monaco-editor/esm/vs/editor/common/languages/supports/tokenization.js';

    interface BuiltinTheme {
        rules: ITokenThemeRule[];
        encodedTokensColors?: string[];
    }
    export const vs: BuiltinTheme;
    export const vs_dark: BuiltinTheme;
}

declare module 'monaco-editor/esm/vs/editor/common/core/editorColorRegistry.js' {
    /** Ids of the editor's bracket pair colorization colors. */
    export const editorBracketHighlightingForeground1: string;
    export const editorBracketHighlightingForeground2: string;
    export const editorBracketHighlightingForeground3: string;
    export const editorBracketHighlightingForeground4: string;
    export const editorBracketHighlightingForeground5: string;
    export const editorBracketHighlightingForeground6: string;
    export const editorBracketHighlightingUnexpectedBracketForeground: string;
}

declare module 'monaco-editor/esm/vs/platform/theme/common/colorUtils.js' {
    export const Extensions: { ColorContribution: string };
}

declare module 'monaco-editor/esm/vs/platform/registry/common/platform.js' {
    import type { Color } from 'monaco-editor/esm/vs/base/common/color.js';

    /** The theme surface the color registry resolves defaults against. */
    export interface ColorTheme {
        type: 'dark' | 'light';
        getColor(id: string): Color | undefined;
    }

    export const Registry: {
        as(id: string): {
            resolveDefaultColor(
                id: string,
                theme: ColorTheme,
            ): Color | undefined;
        };
    };
}
