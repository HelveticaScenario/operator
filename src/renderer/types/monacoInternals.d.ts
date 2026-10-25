// Types for the deep monaco-editor ESM files the renderer imports.
// monaco-editor ships types only for its main entry; these cover just the
// members used.

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

// The bundled TypeScript language adapters, which the JSDoc link providers
// extend to render their documentation.
declare module 'monaco-editor/esm/vs/language/typescript/languageFeatures.js' {
    import type {
        IPosition,
        IRange,
        Uri,
        editor,
        languages,
        typescript,
    } from 'monaco-editor';
    import type { TextSpan } from 'typescript';

    type WorkerAccessor = (
        ...uris: Uri[]
    ) => Promise<typescript.TypeScriptWorker>;

    export class Adapter {
        constructor(worker: WorkerAccessor);
        protected _worker: WorkerAccessor;
        protected _textSpanToRange(
            model: editor.ITextModel,
            span: TextSpan,
        ): IRange;
    }

    export class QuickInfoAdapter
        extends Adapter
        implements languages.HoverProvider
    {
        provideHover(
            model: editor.ITextModel,
            position: IPosition,
        ): Promise<languages.Hover | undefined>;
    }

    export class SuggestAdapter
        extends Adapter
        implements languages.CompletionItemProvider
    {
        static convertKind(kind: string): languages.CompletionItemKind;
        get triggerCharacters(): string[];
        provideCompletionItems(
            model: editor.ITextModel,
            position: IPosition,
        ): Promise<languages.CompletionList | undefined>;
        resolveCompletionItem(
            item: languages.CompletionItem,
        ): Promise<languages.CompletionItem>;
    }

    export class SignatureHelpAdapter
        extends Adapter
        implements languages.SignatureHelpProvider
    {
        static _toSignatureHelpTriggerReason(
            context: languages.SignatureHelpContext,
        ): unknown;
        signatureHelpTriggerCharacters: string[];
        provideSignatureHelp(
            model: editor.ITextModel,
            position: IPosition,
            token: unknown,
            context: languages.SignatureHelpContext,
        ): Promise<languages.SignatureHelpResult | undefined>;
    }
}

declare module 'monaco-editor/esm/vs/platform/commands/common/commands.js' {
    export interface ICommandService {
        executeCommand(id: string, ...args: unknown[]): Promise<unknown>;
    }
    /** Service identifier; pass to a command handler's `accessor.get`. */
    export const ICommandService: { readonly __brand: 'ICommandService' };
}
