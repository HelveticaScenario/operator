import {
    formatHzLiteral,
    parseSliderValue,
    voltsToHz,
    voltsToNoteName,
} from '../../shared/dsl/sliderUnits';
import type { SliderUnit } from '../../shared/dsl/sliderUnits';
import { extractControls, isIncomplete } from '../dsl/extractControls';
import type { SourceRange } from '../dsl/extractControls';

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
 * Replace the range that `computeEdit` locates in the visible buffer's
 * current text. The control panel only shows the visible buffer's controls,
 * so this is always the buffer the control was declared in.
 */
export function rewriteSource(
    activeModel: SliderEditModel | null,
    computeEdit: (source: string) => { span: SourceRange; text: string } | null,
): void {
    if (!activeModel) {
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

/**
 * Format slider volts as a source literal in the slider's own unit, keeping
 * the quote style of the literal it replaces. Also returns the volts that
 * literal parses back to, so the engine and the code agree exactly.
 */
function sliderLiteral(
    unit: SliderUnit,
    volts: number,
    quote: string,
): { text: string; volts: number } {
    if (unit === 'number') {
        const text = Number(volts.toPrecision(6)).toString();
        return { text, volts: Number(text) };
    }
    const body =
        unit === 'hz'
            ? formatHzLiteral(voltsToHz(volts))
            : voltsToNoteName(volts);
    const q = quote === '"' ? '"' : "'";
    return { text: `${q}${body}${q}`, volts: parseSliderValue(body).volts };
}

/**
 * Mirror a slider drag into the visible buffer's `$slider` value literal and,
 * when the slider is live (`moduleId` non-null), into the audio engine. The
 * slider is found by its call position, re-parsed from the current text.
 *
 * @returns The volts written, as the rewritten literal parses back
 */
export function applySliderChange(
    slider: { callStart: number; unit: SliderUnit },
    newValue: number,
    moduleId: string | null,
    activeModel: SliderEditModel | null,
    setModuleParam: (
        moduleId: string,
        moduleType: string,
        params: Record<string, unknown>,
    ) => void,
): number {
    const written = sliderLiteral(slider.unit, newValue, "'").volts;
    rewriteSource(activeModel, (source) => {
        const current = extractControls(source).sliders.find(
            (s) => s.callStart === slider.callStart,
        );
        if (!current || isIncomplete(current)) {
            return null;
        }
        const { text } = sliderLiteral(
            slider.unit,
            newValue,
            source[current.valueRange.start],
        );
        return { span: current.valueRange, text };
    });

    if (moduleId !== null) {
        setModuleParam(moduleId, '$signal', { source: written });
    }
    return written;
}
