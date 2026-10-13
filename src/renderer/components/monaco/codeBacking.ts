import type { editor } from 'monaco-editor';

const backingOptions: editor.IModelDecorationOptions = {
    className: 'code-backing',
    // Drawn first among decorations; the CSS also stacks it beneath the
    // selection and every other highlight.
    zIndex: -1,
};

/**
 * Keeps one decoration over the whole model so each line's text has a box
 * that ends where the text ends. It is only styled while a video picture
 * shows behind the code.
 */
export function trackCodeBacking(ed: editor.ICodeEditor): () => void {
    const collection = ed.createDecorationsCollection();
    const apply = () => {
        const model = ed.getModel();
        collection.set(
            model
                ? [
                      {
                          range: model.getFullModelRange(),
                          options: backingOptions,
                      },
                  ]
                : [],
        );
    };
    apply();
    const content = ed.onDidChangeModelContent(apply);
    const model = ed.onDidChangeModel(apply);
    return () => {
        content.dispose();
        model.dispose();
        collection.clear();
    };
}
