import type { SliderDefinition } from '../../shared/dsl/sliderTypes';
import { findSliderValueSpan } from '../dsl/sliderSourceEdit';

/** Minimal Monaco text-model surface needed to rewrite a slider literal. */
export interface SliderEditModel {
    getValue(): string;
    getPositionAt(offset: number): { lineNumber: number; column: number };
    pushEditOperations(
        beforeCursorState: null,
        editOperations: {
            range: {
                startLineNumber: number;
                startColumn: number;
                endLineNumber: number;
                endColumn: number;
            };
            text: string;
        }[],
        cursorStateComputer: () => null,
    ): unknown;
}

/**
 * Push a slider drag to the audio engine and mirror it into the source text.
 *
 * Slider definitions belong to the running patch, but `activeModel` is the
 * editor's currently visible buffer — which can be a different file. The
 * literal rewrite therefore only happens when `activeModelIsRunning` is true;
 * otherwise only the engine value changes and no document is touched, even if
 * the visible buffer contains a `$slider` call with the same label.
 */
export function applySliderChange(
    slider: SliderDefinition,
    newValue: number,
    activeModel: SliderEditModel | null,
    activeModelIsRunning: boolean,
    setModuleParam: (
        moduleId: string,
        moduleType: string,
        params: Record<string, unknown>,
    ) => void,
): void {
    setModuleParam(slider.moduleId, '$signal', { source: newValue });

    if (!activeModelIsRunning || !activeModel) {
        return;
    }

    const source = activeModel.getValue();
    const span = findSliderValueSpan(source, slider.label);
    if (!span) {
        return;
    }

    const startPos = activeModel.getPositionAt(span.start);
    const endPos = activeModel.getPositionAt(span.end);
    const formattedValue = Number(newValue.toPrecision(6)).toString();
    // pushEditOperations keeps the rewrite on the user's undo stack.
    activeModel.pushEditOperations(
        null,
        [
            {
                range: {
                    endColumn: endPos.column,
                    endLineNumber: endPos.lineNumber,
                    startColumn: startPos.column,
                    startLineNumber: startPos.lineNumber,
                },
                text: formattedValue,
            },
        ],
        () => null,
    );
}
