import type { ButtonMode } from '../../shared/dsl/buttonTypes';
import { GATE_HIGH_VOLTAGE } from '../../shared/dsl/buttonTypes';
import { extractControls, isIncomplete } from '../dsl/extractControls';
import { rewriteSource, type SliderEditModel } from './sliderChange';

/**
 * Apply a button press/release. A live button (`moduleId` non-null) drives
 * the audio engine; a toggle also persists its new state into the visible
 * buffer's `$toggleBtn` literal, live or not, found by its call position. A
 * gate never touches the source.
 */
export function applyButtonChange(
    button: { callStart: number; mode: ButtonMode },
    pressed: boolean,
    moduleId: string | null,
    activeModel: SliderEditModel | null,
    setModuleParam: (
        moduleId: string,
        moduleType: string,
        params: Record<string, unknown>,
    ) => void,
): void {
    if (moduleId !== null) {
        setModuleParam(moduleId, '$signal', {
            source: pressed ? GATE_HIGH_VOLTAGE : 0,
        });
    }

    if (button.mode !== 'toggle') {
        return;
    }

    rewriteSource(activeModel, (source) => {
        const current = extractControls(source).buttons.find(
            (b) => b.callStart === button.callStart,
        );
        if (!current || isIncomplete(current) || !current.stateRange) {
            return null;
        }
        return { span: current.stateRange, text: String(pressed) };
    });
}

/**
 * Persist a group's collapsed state into its `$cGroup` call in the visible
 * buffer, found by its call position: the state literal is replaced, or
 * appended after the label when the call has none.
 */
export function applyGroupCollapse(
    group: { callStart: number },
    collapsed: boolean,
    activeModel: SliderEditModel | null,
): void {
    rewriteSource(activeModel, (source) => {
        const edit = extractControls(source).groups.find(
            (g) => g.callStart === group.callStart,
        )?.collapseEdit;
        return edit
            ? { span: edit.range, text: `${edit.prefix}${String(collapsed)}` }
            : null;
    });
}
