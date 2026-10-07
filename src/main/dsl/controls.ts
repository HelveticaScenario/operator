/**
 * DSL control factories: `$slider`, `$btn`, `$toggleBtn`, and `$cGroup`.
 *
 * Each control is a `$signal` module the control panel drives. `$cGroup`
 * returns an opaque {@link ControlGroup} reference; passing it as a
 * control's (or another group's) optional group argument places that
 * control inside the group. Groups are purely visual (the panel reads them
 * from the source), but a control's group path scopes its label — labels are
 * unique within a group, not across the patch.
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

/**
 * A reference to a control group, as `$cGroup` returns it. Only references
 * issued by the current patch evaluation are accepted as a group argument.
 */
class ControlGroup {
    constructor(readonly label: string) {
        Object.freeze(this);
    }
}

export interface Controls {
    $slider: (
        label: string,
        value: number | string,
        min: number | string,
        max: number | string,
        group?: ControlGroup,
        ...rest: unknown[]
    ) => CollectionWithRange;
    $btn: (
        label: string,
        group?: ControlGroup,
        ...rest: unknown[]
    ) => CollectionWithRange;
    $toggleBtn: (
        label: string,
        initial: boolean,
        group?: ControlGroup,
        ...rest: unknown[]
    ) => CollectionWithRange;
    $cGroup: (
        label: string,
        params?: { collapsed?: boolean; group?: ControlGroup },
        ...rest: unknown[]
    ) => ControlGroup;
}

/** Where a `$cGroup` call created a group, for the placement check. */
export interface GroupDefinition {
    label: string;
    /** Labels of the enclosing groups, outermost first */
    group: string[];
    sourceLocation?: { line: number; column: number };
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

/**
 * True for a plain `{ ... }` literal. Patch scripts evaluate in a vm realm
 * whose `Object.prototype` differs from the host's, so plainness is judged by
 * prototype-chain depth rather than identity.
 */
function isPlainObject(v: unknown): v is Record<string, unknown> {
    if (typeof v !== 'object' || v === null || Array.isArray(v)) {
        return false;
    }
    const proto = Object.getPrototypeOf(v);
    return proto === null || Object.getPrototypeOf(proto) === null;
}

export function createControls(deps: ControlDeps): Controls & {
    sliders: SliderDefinition[];
    buttons: ButtonDefinition[];
    groups: GroupDefinition[];
} {
    const sliders: SliderDefinition[] = [];
    const buttons: ButtonDefinition[] = [];
    const groups: GroupDefinition[] = [];
    const groupKeys = new Set<string>();
    // Every group this evaluation issued, with its label path.
    const groupPaths = new WeakMap<object, string[]>();

    /** The label path of a group argument; [] when it is omitted. */
    const resolveGroup = (fn: string, group: unknown): string[] => {
        if (group === undefined) {
            return [];
        }
        const path =
            typeof group === 'object' && group !== null
                ? groupPaths.get(group)
                : undefined;
        if (!path) {
            throw new Error(`${fn} group must be the result of a $cGroup() call`);
        }
        return path;
    };

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

    return {
        /**
         * Create a slider control: a signal module with a UI slider bound to
         * it. Value/min/max must share one unit: all numbers (volts), all hz
         * strings, or all note strings. For hz/note the stored values are
         * V/Oct volts.
         */
        $slider(label, value, min, max, groupRef, ...rest) {
            if (rest.length > 0) {
                throw new Error(
                    '$slider() takes label, value, min, max, and an optional group',
                );
            }
            const group = resolveGroup('$slider()', groupRef);
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
        $btn(label, groupRef, ...rest) {
            if (rest.length > 0) {
                throw new Error('$btn() takes a label and an optional group');
            }
            const group = resolveGroup('$btn()', groupRef);
            validateLabel('$btn()', group, label);
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
        $toggleBtn(label, initial, groupRef, ...rest) {
            if (rest.length > 0) {
                throw new Error(
                    '$toggleBtn() takes label, initial state, and an optional group',
                );
            }
            const group = resolveGroup('$toggleBtn()', groupRef);
            validateLabel('$toggleBtn()', group, label);
            if (typeof initial !== 'boolean') {
                throw new Error(
                    '$toggleBtn() initial state must be a true or false literal',
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
         * Create a control group: controls given it as their group argument
         * appear together under a collapsible header. `params.collapsed` is
         * the header's state, which the panel rewrites in the source;
         * `params.group` nests this group inside another.
         */
        $cGroup(label, params, ...rest) {
            if (typeof label !== 'string') {
                throw new Error('$cGroup() label must be a string literal');
            }
            if (rest.length > 0) {
                throw new Error(
                    '$cGroup() takes a label and an optional { collapsed, group } object',
                );
            }
            if (params !== undefined && !isPlainObject(params)) {
                throw new Error(
                    '$cGroup() second argument must be a { collapsed, group } object',
                );
            }
            const { collapsed, group: groupRef, ...others } = params ?? {};
            const extra = Object.keys(others);
            if (extra.length > 0) {
                throw new Error(
                    `$cGroup() params accept only collapsed and group, not ${extra.join(', ')}`,
                );
            }
            if (collapsed !== undefined && typeof collapsed !== 'boolean') {
                throw new Error(
                    '$cGroup() collapsed must be a true or false literal',
                );
            }
            const group = resolveGroup('$cGroup()', groupRef);
            const child = [...group, label];
            const key = JSON.stringify(child);
            if (groupKeys.has(key)) {
                throw new Error(
                    `$cGroup() label "${label}" must be unique${inGroup(group)}`,
                );
            }
            groupKeys.add(key);
            groups.push({
                group,
                label,
                sourceLocation: captureSourceLocation(),
            });
            const ref = new ControlGroup(label);
            groupPaths.set(ref, child);
            return ref;
        },
        buttons,
        groups,
        sliders,
    };
}

/**
 * Reject any control or group the static scan does not place where
 * evaluation put it. The control panel builds its controls and groups from
 * the source, so one whose group argument it cannot resolve — a group
 * passed as a parameter or held in a `let` — or one made through an aliased
 * factory would be missing from the panel, and one it resolves differently
 * would show in the wrong group.
 */
export function assertControlsPlaced(
    source: string,
    controls: Array<{
        label: string;
        group: string[];
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
    const placed = new Map(
        findControlCalls(sourceFile).map(({ name, group }) => {
            const { line, character } =
                sourceFile.getLineAndCharacterOfPosition(
                    name.getStart(sourceFile),
                );
            const column =
                character + 1 + (line === 0 ? FIRST_LINE_COLUMN_OFFSET : 0);
            return [`${line + 1}:${column}`, group] as const;
        }),
    );
    for (const { label, group, sourceLocation: loc } of controls) {
        if (!loc) {
            continue;
        }
        const scanned = placed.get(`${loc.line}:${loc.column}`);
        if (!scanned) {
            throw new Error(
                `Control "${label}" at line ${loc.line} must be created by a direct $slider/$btn/$toggleBtn/$cGroup call whose group, if given, is a $cGroup(...) call or a const bound to one ($cGroup params written as an object literal), so the Control panel can place it`,
            );
        }
        if (JSON.stringify(scanned) !== JSON.stringify(group)) {
            throw new Error(
                `Control "${label}" at line ${loc.line} is in group "${group.join(' / ')}" when evaluated, but the Control panel would place it in "${scanned.join(' / ')}" — pass the group as a $cGroup(...) call or a const bound to one that no other binding shadows`,
            );
        }
    }
}
