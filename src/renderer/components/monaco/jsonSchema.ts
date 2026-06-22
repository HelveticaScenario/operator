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

export function registerConfigSchemaForFile(
    monaco: Monaco,
    schema: object,
    currentFile: string,
) {
    const { jsonDefaults } = monaco.json;
    const fileUri = `file://${currentFile}`;
    jsonDefaults.setDiagnosticsOptions({
        allowComments: true,
        trailingCommas: 'ignore',
        schemas: [
            {
                uri: 'modular://config-schema.json',
                fileMatch: ['*'],
                schema,
            },
        ],
        validate: true,
    });
    return fileUri;
}
