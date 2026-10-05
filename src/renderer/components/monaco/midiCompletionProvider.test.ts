/**
 * Tests for the MIDI device completion provider. Drives the real provider
 * with a mock model/position and asserts the insert text, in particular that
 * accepting a suggestion between an auto-closed quote pair yields exactly one
 * closing quote.
 */
import { describe, expect, test } from 'vitest';
import type { languages } from 'monaco-editor';
import { registerMidiCompletionProvider } from './midiCompletionProvider';

const DEVICES = [{ name: 'IAC Driver Bus 1', index: 0 }];

function makeProvider(): languages.CompletionItemProvider {
    let captured: languages.CompletionItemProvider | undefined;
    const monaco = {
        languages: {
            CompletionItemKind: { Value: 13 },
            registerCompletionItemProvider: (
                _lang: string,
                p: languages.CompletionItemProvider,
            ) => {
                captured = p;
                return { dispose: () => {} };
            },
        },
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
    } as any;
    registerMidiCompletionProvider(monaco, async () => DEVICES);
    return captured!;
}

/** Run the provider on a single line with the cursor at `column`. */
async function complete(
    lineContent: string,
    column: number,
    wordAtPosition: { word: string; startColumn: number; endColumn: number } | null = null,
): Promise<languages.CompletionList | undefined> {
    const provider = makeProvider();
    const model = {
        getLineContent: () => lineContent,
        getWordAtPosition: () => wordAtPosition,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
    } as any;
    return provider.provideCompletionItems(
        model,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        { lineNumber: 1, column } as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {} as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        {} as any,
    ) as Promise<languages.CompletionList | undefined>;
}

describe('midiCompletionProvider insert text', () => {
    test('does not append a closing quote when auto-close already inserted one', async () => {
        // Typing `"` after `device: ` auto-closes to `""` with the cursor in
        // between. Accepting must produce `"IAC Driver Bus 1"`, not `..."".
        const line = '$midi({ device: "" })';
        const result = await complete(line, 18);
        expect(result?.suggestions).toHaveLength(1);
        expect(result!.suggestions[0].insertText).toBe('IAC Driver Bus 1');
    });

    test('appends the closing quote when the buffer has none', async () => {
        const line = '$midi({ device: "';
        const result = await complete(line, 18);
        expect(result?.suggestions).toHaveLength(1);
        expect(result!.suggestions[0].insertText).toBe('IAC Driver Bus 1"');
    });

    test('matches the opening quote style when closing a single-quoted string', async () => {
        const line = "$midi({ device: '";
        const result = await complete(line, 18);
        expect(result?.suggestions).toHaveLength(1);
        expect(result!.suggestions[0].insertText).toBe("IAC Driver Bus 1'");
    });

    test('replacing a partial word keeps the existing closing quote', async () => {
        // Cursor right after the opening quote with `IAC"` already present:
        // the range replaces the word, and the quote after it must survive
        // as the only closing quote.
        const line = '$midi({ device: "IAC" })';
        const result = await complete(line, 18, {
            word: 'IAC',
            startColumn: 18,
            endColumn: 21,
        });
        expect(result?.suggestions).toHaveLength(1);
        expect(result!.suggestions[0].insertText).toBe('IAC Driver Bus 1');
    });

    test('wraps the name in quotes when none are typed yet', async () => {
        const line = '$midi({ device: ';
        const result = await complete(line, 17);
        expect(result?.suggestions).toHaveLength(1);
        expect(result!.suggestions[0].insertText).toBe('"IAC Driver Bus 1"');
    });
});
