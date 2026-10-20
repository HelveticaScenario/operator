import type { ButtonDefinition } from '../../shared/dsl/buttonTypes';
import type { PlacedControl } from '../performance/PerformanceCode';
import type { ButtonView, SliderView } from './controlBinding';
import { sliderPositionCount, voltsToPosition } from './sliderPositions';

/**
 * The performance view's widgets for the visible buffer's controls, in the
 * order of their calls. A gate shows its running control's state, since its
 * pressed state is not in the code.
 */
export function performanceControls(
    sliders: SliderView[],
    buttons: ButtonView[],
    runningButtons: ButtonDefinition[],
): PlacedControl[] {
    const held = new Set(
        runningButtons.filter((b) => b.value).map((b) => b.moduleId),
    );
    return [
        ...sliders.map(
            (s): PlacedControl => ({
                fraction: voltsToPosition(s.value, s) / sliderPositionCount(s),
                kind: 'slider',
                offset: s.callStart,
            }),
        ),
        ...buttons.map(
            (b): PlacedControl => ({
                kind: 'button',
                offset: b.callStart,
                on:
                    b.mode === 'toggle'
                        ? b.value
                        : b.moduleId !== null && held.has(b.moduleId),
            }),
        ),
    ].sort((a, b) => a.offset - b.offset);
}
