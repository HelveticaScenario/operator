import type { Monaco } from '../../hooks/useCustomMonaco';

// Apply the generated DSL .d.ts library to Monaco and expose some
// Debug handles on window so we can inspect schemas and lib source
// From the browser console.
export function applyDslLibToMonaco(monaco: Monaco, libSource: string) {
    if (!monaco || !libSource) {
        return {};
    }

    const ts = monaco.typescript;
    const jsDefaults = ts.javascriptDefaults;
    const extraLib = jsDefaults.addExtraLib(
        libSource,
        'file:///modular/dsl-lib.d.ts',
    );
    const extraLibModel = monaco.editor.createModel(
        libSource,
        'typescript',
        monaco.Uri.parse('file:///modular/dsl-lib.d.ts'),
    );
    extraLibModel.onDidChangeContent((_e) => {
        // TODO: Make this model read-only
    });
    return { extraLib, extraLibModel };
}

export function formatPath(currentFile: string) {
    if (!currentFile.startsWith('/')) {
        currentFile = '/' + currentFile;
    }
    // Append .mjs so extensionless/untitled buffers are treated as ES modules
    // by Monaco's TypeScript tooling. Real extensions are left intact — in
    // particular .json keeps a JSON model URI (keybindings.json, config.json),
    // rather than becoming a misleading .json.mjs.
    if (
        !currentFile.endsWith('.js') &&
        !currentFile.endsWith('.mjs') &&
        !currentFile.endsWith('.json')
    ) {
        currentFile += '.mjs';
    }
    return `file://${currentFile}`;
}
