import * as prettier from 'prettier/standalone';
import * as prettierBabel from 'prettier/plugins/babel';
import * as prettierEstree from 'prettier/plugins/estree';
import type { Monaco } from '../../hooks/useCustomMonaco';
import type { PrettierConfig } from '../../../shared/ipcTypes';

export const DEFAULT_PRETTIER_OPTIONS = {
    printWidth: 60,
    semi: false,
    singleQuote: true,
    tabWidth: 2,
    trailingComma: 'all' as const,
};

/** The editor's effective Prettier options: the user's over the defaults. */
export function resolvePrettierOptions(userConfig: PrettierConfig = {}) {
    return { ...DEFAULT_PRETTIER_OPTIONS, ...userConfig };
}

export function registerDslFormattingProvider(
    monaco: Monaco,
    userConfig: PrettierConfig = {},
) {
    return monaco.languages.registerDocumentFormattingEditProvider(
        'javascript',
        {
            async provideDocumentFormattingEdits(model) {
                const formatted = await prettier.format(model.getValue(), {
                    ...resolvePrettierOptions(userConfig),
                    // Parser and plugins must not be overridden
                    parser: 'babel',
                    plugins: [prettierBabel, prettierEstree as any],
                });

                return [
                    {
                        range: model.getFullModelRange(),
                        text: formatted.trimEnd(),
                    },
                ];
            },
        },
    );
}
