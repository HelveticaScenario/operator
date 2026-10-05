import type { SliderDefinition } from '../../shared/dsl/sliderTypes';
import {
    formatHzLiteral,
    voltsToHz,
    voltsToNoteName,
} from '../../shared/dsl/sliderUnits';
import { findSliderValueSpan } from '../dsl/sliderSourceEdit';
import type { SourceSpanResult } from '../dsl/sliderSourceEdit';

/** Minimal Monaco text-model surface needed to rewrite a control literal. */
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
 * Replace the literal that `computeEdit` locates in the running patch's
 * source.
 *
 * Control definitions belong to the running patch, but `activeModel` is the
 * editor's currently visible buffer — which can be a different file. The
 * rewrite therefore only happens when `activeModelIsRunning` is true;
 * otherwise no document is touched, even if the visible buffer contains a
 * control call with the same label.
 */
export function rewriteRunningSource(
    activeModel: SliderEditModel | null,
    activeModelIsRunning: boolean,
    computeEdit: (
        source: string,
    ) => { span: SourceSpanResult; text: string } | null,
): void {
    if (!activeModelIsRunning || !activeModel) {
        return;
    }

    const edit = computeEdit(activeModel.getValue());
    if (!edit) {
        return;
    }

    const startPos = activeModel.getPositionAt(edit.span.start);
    const endPos = activeModel.getPositionAt(edit.span.end);
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
                text: edit.text,
            },
        ],
        () => null,
    );
}

/** Format slider volts as a source literal in the slider's own unit,
 *  keeping the quote style of the literal it replaces. */
function formatSliderLiteral(
    slider: SliderDefinition,
    volts: number,
    quote: string,
): string {
    const q = quote === '"' ? '"' : "'";
    if (slider.unit === 'hz') {
        return `${q}${formatHzLiteral(voltsToHz(volts))}${q}`;
    }
    if (slider.unit === 'note') {
        return `${q}${voltsToNoteName(volts)}${q}`;
    }
    return Number(volts.toPrecision(6)).toString();
}

/**
 * Push a slider drag to the audio engine and mirror it into the source text
 * of the running patch (see {@link rewriteRunningSource}).
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

    rewriteRunningSource(activeModel, activeModelIsRunning, (source) => {
        const span = findSliderValueSpan(source, slider.label);
        if (!span) {
            return null;
        }
        return {
            span,
            text: formatSliderLiteral(slider, newValue, source[span.start]),
        };
    });
}
