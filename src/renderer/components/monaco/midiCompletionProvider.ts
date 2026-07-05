/**
 * Monaco completion provider for MIDI device names.
 * Provides autocomplete suggestions for the `device` parameter in MIDI modules.
 */
import type { Monaco } from '../../hooks/useCustomMonaco';
import type {
    CancellationToken,
    IRange,
    Position,
    editor,
    languages,
} from 'monaco-editor';

/**
 * Function to fetch MIDI device list from the Electron main process.
 */
export type MidiDeviceFetcher = () => Promise<
    { name: string; index: number }[]
>;

/**
 * Creates and registers a completion provider for MIDI device names.
 * The provider triggers when the user types a quote inside a `device:` property.
 *
 * @param monaco Monaco instance
 * @param fetchMidiDevices Function to fetch available MIDI devices
 * @returns Disposable to unregister the provider
 */
export function registerMidiCompletionProvider(
    monaco: Monaco,
    fetchMidiDevices: MidiDeviceFetcher,
): { dispose: () => void } {
    const provider: languages.CompletionItemProvider = {
        // Trigger on quotes and after typing
        async provideCompletionItems(
            model: editor.ITextModel,
            position: Position,
            _context: languages.CompletionContext,
            _token: CancellationToken,
        ): Promise<languages.CompletionList | undefined> {
            // Get the text before the cursor on the current line
            const lineContent = model.getLineContent(position.lineNumber);
            const textBeforeCursor = lineContent.substring(
                0,
                position.column - 1,
            );

            // Check if we're in a device property context
            // Patterns to match:
            // - device: "
            // - device: '
            // - device:"
            // - device:'
            // - { device: "
            const devicePatterns = [
                /device\s*:\s*["']$/, // device: " or device: '
                /device\s*:\s*$/, // device:  (just typed colon)
                /,\s*device\s*:\s*["']?$/, // , device: " (in object)
                /{\s*device\s*:\s*["']?$/, // { device: " (start of object)
            ];

            const isDeviceContext = devicePatterns.some((pattern) =>
                pattern.test(textBeforeCursor),
            );

            if (!isDeviceContext) {
                return undefined;
            }

            // Fetch available MIDI devices
            let midiInputs: Array<{ name: string; index: number }>;
            try {
                midiInputs = await fetchMidiDevices();
            } catch (error) {
                console.error(
                    '[MIDI Completion] Failed to fetch devices:',
                    error,
                );
                return undefined;
            }

            if (midiInputs.length === 0) {
                return undefined;
            }

            // Determine if we're already inside quotes
            const endsWithQuote =
                textBeforeCursor.endsWith('"') ||
                textBeforeCursor.endsWith("'");
            const quoteChar = textBeforeCursor.endsWith('"') ? '"' : "'";

            // Determine the range for the completion
            const wordAtPosition = model.getWordAtPosition(position);
            const range: IRange = {
                startLineNumber: position.lineNumber,
                endLineNumber: position.lineNumber,
                startColumn: wordAtPosition?.startColumn ?? position.column,
                endColumn: wordAtPosition?.endColumn ?? position.column,
            };

            // Monaco's auto-closing pairs insert the closing quote as soon as
            // the opening one is typed, and completion insertion does not
            // overtype it — so when a closing quote already follows the
            // replaced range, the insert text must not add another.
            const hasClosingQuote =
                lineContent.charAt(range.endColumn - 1) === quoteChar;

            // Build suggestions
            const suggestions: languages.CompletionItem[] = midiInputs.map(
                (device, index) => {
                    // Determine what to insert
                    let insertText: string;
                    if (endsWithQuote) {
                        // Already typed opening quote; close it only when the
                        // buffer has not already
                        insertText = hasClosingQuote
                            ? device.name
                            : device.name + quoteChar;
                    } else {
                        // Need to add quotes around the name
                        insertText = `"${device.name}"`;
                    }

                    return {
                        label: device.name,
                        kind: monaco.languages.CompletionItemKind.Value,
                        detail: `MIDI Input #${device.index}`,
                        documentation: {
                            value: `Connect to MIDI device: **${device.name}**`,
                        },
                        insertText,
                        range,
                        sortText: String(index).padStart(3, '0'), // Preserve order from system
                    };
                },
            );

            return {
                suggestions,
                incomplete: false, // List is complete
            };
        },

        triggerCharacters: ['"', "'", ':'],
    };

    const disposable = monaco.languages.registerCompletionItemProvider(
        'javascript',
        provider,
    );

    return {
        dispose: () => disposable.dispose(),
    };
}
