import type { ButtonDefinition } from '../../shared/dsl/buttonTypes';
import type { SliderDefinition } from '../../shared/dsl/sliderTypes';
import type {
    ResolvedControls,
    StaticButton,
    StaticSlider,
} from '../dsl/extractControls';

/**
 * A control as the panel shows it: declared by the visible buffer's code,
 * bound to the running patch's control for the same call.
 */
export interface ControlBinding {
    /** Backing module of the running patch's control, or null when the
     *  control is not live (its buffer is not running, or evaluation has not
     *  created it yet). Only a live control drives the audio engine. */
    moduleId: string | null;
    /** True when the running patch's control matches the code exactly. */
    synced: boolean;
    /** True while the control's call has invalid arguments; the view then
     *  shows the control's last valid state and is neither live nor usable. */
    incomplete: boolean;
}

export type SliderView = StaticSlider & ControlBinding;
export type ButtonView = StaticButton & ControlBinding;

const EPSILON = 1e-9;

function sameVolts(a: number, b: number): boolean {
    return Math.abs(a - b) < EPSILON;
}

function sliderSynced(code: StaticSlider, running: SliderDefinition): boolean {
    return (
        code.unit === running.unit &&
        sameVolts(code.value, running.value) &&
        sameVolts(code.min, running.min) &&
        sameVolts(code.max, running.max)
    );
}

function buttonSynced(code: StaticButton, running: ButtonDefinition): boolean {
    // A gate's value tracks the pointer, not the code, so only mode counts.
    return (
        code.mode === running.mode &&
        (code.mode === 'gate' || code.value === running.value)
    );
}

/**
 * Pair each code control with the running control it belongs to. A running
 * control with a live anchor binds to the code control whose call starts at
 * the anchor, so editing a label keeps the binding; one without an anchor
 * falls back to matching by label within the same group.
 */
function matchRunning<
    C extends { label: string; group: string[]; callStart: number },
    R extends { label: string; group: string[]; moduleId: string },
>(
    code: C[],
    running: R[],
    anchorOffset: (moduleId: string) => number | null,
): (R | undefined)[] {
    const byOffset = new Map<number, R>();
    const unanchored: R[] = [];
    for (const r of running) {
        const offset = anchorOffset(r.moduleId);
        if (offset === null) {
            unanchored.push(r);
        } else {
            byOffset.set(offset, r);
        }
    }
    return code.map((c) => {
        const anchored = byOffset.get(c.callStart);
        if (anchored) {
            return anchored;
        }
        const i = unanchored.findIndex(
            (r) =>
                r.label === c.label &&
                r.group.length === c.group.length &&
                r.group.every((label, j) => label === c.group[j]),
        );
        return i === -1 ? undefined : unanchored.splice(i, 1)[0];
    });
}

/**
 * Bind the controls declared in the visible buffer to the running patch's
 * controls. `isRunningBuffer` is false when the visible buffer is not the one
 * the running patch was evaluated from; its controls are then never live.
 * Incomplete controls are never live either. `anchorOffset` gives a running
 * control's current call offset in the visible buffer (see ControlAnchors).
 */
export function bindControls(
    code: ResolvedControls,
    runningSliders: SliderDefinition[],
    runningButtons: ButtonDefinition[],
    isRunningBuffer: boolean,
    anchorOffset: (moduleId: string) => number | null,
): { sliders: SliderView[]; buttons: ButtonView[] } {
    const sliderMatches = isRunningBuffer
        ? matchRunning(code.sliders, runningSliders, anchorOffset)
        : [];
    const buttonMatches = isRunningBuffer
        ? matchRunning(code.buttons, runningButtons, anchorOffset)
        : [];

    const sliders = code.sliders.map((slider, i): SliderView => {
        const running = slider.incomplete ? undefined : sliderMatches[i];
        return {
            ...slider,
            moduleId: running?.moduleId ?? null,
            synced: running !== undefined && sliderSynced(slider, running),
        };
    });
    const buttons = code.buttons.map((button, i): ButtonView => {
        const running = button.incomplete ? undefined : buttonMatches[i];
        return {
            ...button,
            moduleId: running?.moduleId ?? null,
            synced: running !== undefined && buttonSynced(button, running),
        };
    });
    return { buttons, sliders };
}
