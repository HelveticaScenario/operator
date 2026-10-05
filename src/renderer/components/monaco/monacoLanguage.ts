import type { Monaco } from '../../hooks/useCustomMonaco';
import { applyDslLibToMonaco } from './monacoHelpers';
import type { ModuleSchema } from '@modular/core';

export interface MonacoSetupOptions {
    /** Module schemas for building symbol sets */
    schemas?: ModuleSchema[];
}

/**
 * Bracket and quote pairs matching VS Code's JavaScript language configuration.
 * Monaco's bundled JavaScript configuration declares no `surroundingPairs`, so
 * the editor falls back to `autoClosingPairs` — which omits `<`/`>` and
 * therefore replaces a selection rather than wrapping it. Declaring the pairs
 * restores wrapping for `<>` alongside `()`, `[]`, and `{}`, which
 * mini-notation uses for grouping inside pattern strings.
 *
 * Colorization is left unconfigured: with no `colorizedBracketPairs`, the
 * editor excludes `<`/`>` from bracket-pair colors, so comparison operators
 * keep the operator color. Configuring the field opts them in — which is why
 * VS Code colorizes angle brackets in TypeScript but not in JavaScript.
 */
function configureJavascriptPairs(monaco: Monaco) {
    return monaco.languages.setLanguageConfiguration('javascript', {
        autoCloseBefore: ';:.,=}])>` \n\t',
        autoClosingPairs: [
            { close: '}', open: '${' },
            { close: '}', open: '{' },
            { close: ']', open: '[' },
            { close: ')', open: '(' },
            { close: "'", notIn: ['string', 'comment'], open: "'" },
            { close: '"', notIn: ['string'], open: '"' },
            { close: '`', notIn: ['string', 'comment'], open: '`' },
            { close: ' */', notIn: ['string'], open: '/**' },
        ],
        // Typing `$` then `{` around a selection produces `${selection}`; the
        // `${` entry never matches on its own, since surrounding keys off a
        // single typed character.
        surroundingPairs: [
            { close: '}', open: '${' },
            { close: '', open: '$' },
            { close: '}', open: '{' },
            { close: ']', open: '[' },
            { close: ')', open: '(' },
            { close: "'", open: "'" },
            { close: '"', open: '"' },
            { close: '`', open: '`' },
            { close: '>', open: '<' },
        ],
    });
}

export function setupMonacoJavascript(
    monaco: Monaco,
    libSource: string,
    _options: MonacoSetupOptions = {},
) {
    const ts = monaco.typescript;
    console.log('Monaco TS version:', ts);
    const jsDefaults = ts.javascriptDefaults;

    jsDefaults.setCompilerOptions({
        allowJs: true,
        allowNonTsExtensions: true,
        checkJs: true,
        lib: ['esnext'],
        module: ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.NodeJs,
        noEmit: true,
        target: ts.ScriptTarget.ES2020,
    });

    jsDefaults.setDiagnosticsOptions({
        noSemanticValidation: false,
        noSyntaxValidation: false,
    });

    jsDefaults.setEagerModelSync(true);

    const languageConfiguration = configureJavascriptPairs(monaco);
    const { extraLib, extraLibModel } = applyDslLibToMonaco(monaco, libSource);

    return () => {
        languageConfiguration.dispose();
        extraLib?.dispose();
        extraLibModel?.dispose();
    };
}
