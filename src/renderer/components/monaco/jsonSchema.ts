import type { Monaco } from '../../hooks/useCustomMonaco';

export function registerConfigSchema(monaco: Monaco, schema: object) {
    const { jsonDefaults } = monaco.json;
    jsonDefaults.setDiagnosticsOptions({
        // Operator's JSON buffers (keybindings.json, config.json) are JSONC:
        // tolerate comments and trailing commas so they don't show as errors.
        allowComments: true,
        trailingCommas: 'ignore',
        schemas: [
            {
                uri: 'modular://config-schema.json',
                // These globs match config.json's model URI in both the dev
                // ('file:///config.json') and packaged (userData absolute path)
                // forms, and crucially do NOT match keybindings.json, so the
                // config-object schema never validates the keybindings array.
                fileMatch: [
                    '*/config.json',
                    '**/config.json',
                    'config.json',
                    '*.config.json',
                ],
                schema,
            },
        ],
        validate: true,
    });
}
