/**
 * DSL control factories: `$slider`, `$btn`, `$toggleBtn`, and `$cGroup`.
 *
 * Each control is a `$signal` module the control panel drives. `$cGroup`
 * returns a group whose `slider` / `btn` / `toggleBtn` / `cGroup` methods are
 * the same factories bound to a nested group path. Groups are purely visual
 * (the panel reads them from the source), but a control's group path scopes
 * its label — labels are unique within a group, not across the patch.
 */

import type { ButtonDefinition } from '../../shared/dsl/buttonTypes';
import { GATE_HIGH_VOLTAGE } from '../../shared/dsl/buttonTypes';
import type { SliderDefinition } from '../../shared/dsl/sliderTypes';
import { parseSliderValue } from '../../shared/dsl/sliderUnits';
import { findControlCalls } from '../../shared/dsl/controlCalls';
import { FIRST_LINE_COLUMN_OFFSET } from '../../shared/dsl/spanTypes';
import { ts } from 'ts-morph';
import { captureSourceLocation } from './factories';
import type { CollectionWithRange } from './GraphBuilder';

export interface ControlDeps {
    /**
     * Create a constant `$signal` module with an explicit id, wrapped as a
     * collection carrying `[min, max]`.
     */
    rangedSignal(
        value: number,
        id: string,
        min: number,
        max: number,
    ): CollectionWithRange;
}

/** The control factories, bound to one group: what `$cGroup` returns. */
export interface ControlGroupApi {
    slider: (
        label: string,
        value: number | string,
        min: number | string,
        max: number | string,
    ) => CollectionWithRange;
    btn: (label: string, ...rest: unknown[]) => CollectionWithRange;
    toggleBtn: (
        label: string,
        initial: boolean,
        ...rest: unknown[]
    ) => CollectionWithRange;
    cGroup: (
        label: string,
        collapsed?: boolean,
        ...rest: unknown[]
    ) => ControlGroupApi;
}

/** The root group's factories, as the patch's free functions. */
export interface RootControls {
    $slider: ControlGroupApi['slider'];
    $btn: ControlGroupApi['btn'];
    $toggleBtn: ControlGroupApi['toggleBtn'];
    $cGroup: ControlGroupApi['cGroup'];
}

/**
 * Module id for a control. Percent-encoding each path segment keeps the id
 * injective: distinct (group path, label) pairs never collide.
 */
function controlModuleId(
    prefix: string,
    group: string[],
    label: string,
): string {
    return prefix + [...group, label].map(encodeURIComponent).join('/');
}

function inGroup(group: string[]): string {
    return group.length > 0 ? ` in group "${group.join(' / ')}"` : '';
}

export function createControls(deps: ControlDeps): RootControls & {
    sliders: SliderDefinition[];
    buttons: ButtonDefinition[];
} {
    const sliders: SliderDefinition[] = [];
    const buttons: ButtonDefinition[] = [];
    const groupKeys = new Set<string>();

    const sameGroup = (a: string[], b: string[]) =>
        a.length === b.length && a.every((segment, i) => segment === b[i]);

    /** Label checks shared by every control factory. */
    const validateLabel = (fn: string, group: string[], label: string) => {
        if (typeof label !== 'string') {
            throw new Error(`${fn} label must be a string literal`);
        }
        const taken = (c: { label: string; group: string[] }) =>
            c.label === label && sameGroup(c.group, group);
        const isSlider = fn === '$slider()';
        if ((isSlider ? sliders : buttons).some(taken)) {
            throw new Error(
                `${fn} label "${label}" must be unique${inGroup(group)}`,
            );
        }
        if ((isSlider ? buttons : sliders).some(taken)) {
            throw new Error(
                `${fn} label "${label}" is already used by ${isSlider ? 'a button' : 'a $slider()'}${inGroup(group)}`,
            );
        }
    };

    const makeGroup = (group: string[]): ControlGroupApi => ({
        /**
         * Create a slider control: a signal module with a UI slider bound to
         * it. Value/min/max must share one unit: all numbers (volts), all hz
         * strings, or all note strings. For hz/note the stored values are
         * V/Oct volts.
         */
        slider(label, value, min, max) {
            validateLabel('$slider()', group, label);
            // Name the slider and the offending argument so an error among
            // many sliders points at the right literal.
            const parseArg = (
                arg: 'value' | 'min' | 'max',
                v: number | string,
            ) => {
                try {
                    return parseSliderValue(v);
                } catch (err) {
                    throw new Error(
                        `$slider("${label}") ${arg}: ${err instanceof Error ? err.message : String(err)}`,
                        { cause: err },
                    );
                }
            };
            const parsedValue = parseArg('value', value);
            const parsedMin = parseArg('min', min);
            const parsedMax = parseArg('max', max);
            if (
                parsedValue.unit !== parsedMin.unit ||
                parsedValue.unit !== parsedMax.unit
            ) {
                throw new Error(
                    '$slider() value, min, and max must all be numbers, all hz strings, or all note strings',
                );
            }
            if (parsedMin.volts >= parsedMax.volts) {
                throw new Error(
                    `$slider() min (${min}) must be less than max (${max})`,
                );
            }

            const moduleId = controlModuleId('__slider_', group, label);

            sliders.push({
                group,
                label,
                max: parsedMax.volts,
                min: parsedMin.volts,
                moduleId,
                sourceLocation: captureSourceLocation(),
                unit: parsedValue.unit,
                value: parsedValue.volts,
            });
            return deps.rangedSignal(
                parsedValue.volts,
                moduleId,
                parsedMin.volts,
                parsedMax.volts,
            );
        },

        /**
         * Create a momentary gate button: a signal module the UI drives to 5V
         * while the button is held and 0V on release. Chain through .$.hold
         * for fixed-length triggers.
         */
        btn(label, ...rest) {
            validateLabel('$btn()', group, label);
            if (rest.length > 0) {
                throw new Error('$btn() takes only a label argument');
            }
            const moduleId = controlModuleId('__button_', group, label);
            buttons.push({
                group,
                label,
                mode: 'gate',
                moduleId,
                sourceLocation: captureSourceLocation(),
                value: false,
            });
            return deps.rangedSignal(0, moduleId, 0, GATE_HIGH_VOLTAGE);
        },

        /**
         * Create a latched toggle button: clicking flips between 0V and 5V
         * and rewrites the initial-state literal in the source.
         */
        toggleBtn(label, initial, ...rest) {
            validateLabel('$toggleBtn()', group, label);
            if (typeof initial !== 'boolean') {
                throw new Error(
                    '$toggleBtn() initial state must be a true or false literal',
                );
            }
            if (rest.length > 0) {
                throw new Error(
                    '$toggleBtn() takes only label and initial-state arguments',
                );
            }
            const moduleId = controlModuleId('__button_', group, label);

            buttons.push({
                group,
                label,
                mode: 'toggle',
                moduleId,
                sourceLocation: captureSourceLocation(),
                value: initial,
            });
            return deps.rangedSignal(
                initial ? GATE_HIGH_VOLTAGE : 0,
                moduleId,
                0,
                GATE_HIGH_VOLTAGE,
            );
        },

        /**
         * Create a control group nested in this one. Its controls appear
         * together under a collapsible header; `collapsed` is the header's
         * state, which the panel rewrites in the source.
         */
        cGroup(label, collapsed, ...rest) {
            if (typeof label !== 'string') {
                throw new Error('$cGroup() label must be a string literal');
            }
            if (collapsed !== undefined && typeof collapsed !== 'boolean') {
                throw new Error(
                    '$cGroup() collapsed state must be a true or false literal',
                );
            }
            if (rest.length > 0) {
                throw new Error(
                    '$cGroup() takes only label and collapsed-state arguments',
                );
            }
            const child = [...group, label];
            const key = JSON.stringify(child);
            if (groupKeys.has(key)) {
                throw new Error(
                    `$cGroup() label "${label}" must be unique${inGroup(group)}`,
                );
            }
            groupKeys.add(key);
            return makeGroup(child);
        },
    });

    const root = makeGroup([]);
    return {
        $btn: root.btn,
        $cGroup: root.cGroup,
        $slider: root.slider,
        $toggleBtn: root.toggleBtn,
        buttons,
        sliders,
    };
}

/**
 * Reject any control created at a call site the static scan cannot place.
 * The control panel builds its controls and groups from the source, so a
 * control made through a group it cannot see — a group passed as a
 * parameter, held in a `let`, or an aliased factory — would be missing from
 * the panel.
 */
export function assertControlsPlaced(
    source: string,
    controls: Array<{
        label: string;
        sourceLocation?: { line: number; column: number };
    }>,
): void {
    const sourceFile = ts.createSourceFile(
        'patch.js',
        source,
        ts.ScriptTarget.Latest,
        false,
        ts.ScriptKind.JS,
    );
    // Keys in V8 call-site coordinates, matching captured source locations.
    const placed = new Set(
        findControlCalls(sourceFile).map(({ name }) => {
            const { line, character } =
                sourceFile.getLineAndCharacterOfPosition(
                    name.getStart(sourceFile),
                );
            const column =
                character + 1 + (line === 0 ? FIRST_LINE_COLUMN_OFFSET : 0);
            return `${line + 1}:${column}`;
        }),
    );
    for (const { label, sourceLocation: loc } of controls) {
        if (loc && !placed.has(`${loc.line}:${loc.column}`)) {
            throw new Error(
                `Control "${label}" at line ${loc.line} must be created by a $slider/$btn/$toggleBtn call, or by a method call on a $cGroup(...) call or a const bound to one, so the Control panel can place it`,
            );
        }
    }
}
