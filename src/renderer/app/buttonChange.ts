import type { ButtonDefinition } from '../../shared/dsl/buttonTypes';
import { GATE_HIGH_VOLTAGE } from '../../shared/dsl/buttonTypes';
import { findToggleBtnStateSpan } from '../dsl/buttonSourceEdit';
import { rewriteRunningSource, type SliderEditModel } from './sliderChange';

/**
 * Push a button press/release to the audio engine. A toggle also persists its
 * new state into the running patch's `$toggleBtn` literal (see
 * {@link rewriteRunningSource}); a momentary gate never touches the source.
 */
export function applyButtonChange(
    button: ButtonDefinition,
    pressed: boolean,
    activeModel: SliderEditModel | null,
    activeModelIsRunning: boolean,
    setModuleParam: (
        moduleId: string,
        moduleType: string,
        params: Record<string, unknown>,
    ) => void,
): void {
    setModuleParam(button.moduleId, '$signal', {
        source: pressed ? GATE_HIGH_VOLTAGE : 0,
    });

    if (button.mode !== 'toggle') {
        return;
    }

    rewriteRunningSource(activeModel, activeModelIsRunning, (source) => {
        const span = findToggleBtnStateSpan(source, button.label);
        return span ? { span, text: String(pressed) } : null;
    });
}
