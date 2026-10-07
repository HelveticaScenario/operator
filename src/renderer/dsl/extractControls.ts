/**
 * Statically extract the controls and control groups declared in DSL source,
 * so the control panel can show a buffer's controls before its patch is
 * evaluated. The analyzer requires every control argument to be a literal and
 * every control's group to be statically resolvable, so a parse recovers
 * exactly what evaluation would produce.
 *
 * A labelled call that evaluation would reject (wrong arity, a non-literal
 * or unparsable argument, mixed slider units, min >= max) is reported as
 * incomplete, so the panel can keep showing the control while an argument is
 * mid-edit. Within a group, duplicate labels after the first are omitted.
 */

import { ts } from 'ts-morph';
import type { ButtonDefinition } from '../../shared/dsl/buttonTypes';
import {
    findControlCalls,
    GROUP_ARG_INDEX,
    propertyName,
} from '../../shared/dsl/controlCalls';
import type { SliderDefinition } from '../../shared/dsl/sliderTypes';
import { parseSliderValue } from '../../shared/dsl/sliderUnits';
import {
    appendObjectArgument,
    insertObjectProperty,
} from './objectPropertyInsert';
import type { CodeStyle } from './objectPropertyInsert';

/** A `[start, end)` character range in the parsed source. */
export interface SourceRange {
    start: number;
    end: number;
}

/** Where a control's call sits in the parsed source. */
interface CallSite {
    /** Offset of the callee name (`$slider`, …) */
    callStart: number;
    /** Offset just inside the call's opening paren */
    argsStart: number;
}

export type StaticSlider = Omit<
    SliderDefinition,
    'moduleId' | 'sourceLocation'
> &
    CallSite & {
        /** The value literal, which a slider drag rewrites */
        valueRange: SourceRange;
    };

export type StaticButton = Omit<
    ButtonDefinition,
    'moduleId' | 'sourceLocation'
> &
    CallSite & {
        /** A toggle's state literal, which a click rewrites; null for a gate */
        stateRange: SourceRange | null;
    };

export interface StaticGroup extends CallSite {
    label: string;
    /** Labels of the enclosing groups, outermost first */
    group: string[];
    collapsed: boolean;
    /** Whether {@link groupCollapseEdit} can rewrite the collapsed state:
     *  false when the call's params are not a literal it can edit. */
    collapsible: boolean;
}

/** A control call with a literal label whose other arguments are invalid. */
export interface IncompleteControl extends CallSite {
    label: string;
    group: string[];
    incomplete: true;
}

export interface StaticControls {
    sliders: Array<StaticSlider | IncompleteControl>;
    buttons: Array<StaticButton | IncompleteControl>;
    groups: StaticGroup[];
}

/** Controls ready for the panel; `incomplete` entries show their last valid
 *  state while their call is mid-edit. */
export interface ResolvedControls {
    sliders: Array<StaticSlider & { incomplete: boolean }>;
    buttons: Array<StaticButton & { incomplete: boolean }>;
    groups: StaticGroup[];
}

function rangeOf(node: ts.Node, sourceFile: ts.SourceFile): SourceRange {
    return { end: node.getEnd(), start: node.getStart(sourceFile) };
}

/** A numeric (optionally negated) or string literal's value, else null. */
function sliderLiteral(node: ts.Expression): number | string | null {
    if (ts.isNumericLiteral(node)) {
        return Number(node.text);
    }
    if (ts.isStringLiteral(node)) {
        return node.text;
    }
    if (
        ts.isPrefixUnaryExpression(node) &&
        node.operator === ts.SyntaxKind.MinusToken &&
        ts.isNumericLiteral(node.operand)
    ) {
        return -Number(node.operand.text);
    }
    return null;
}

function booleanLiteral(node: ts.Expression): boolean | null {
    if (node.kind === ts.SyntaxKind.TrueKeyword) {
        return true;
    }
    if (node.kind === ts.SyntaxKind.FalseKeyword) {
        return false;
    }
    return null;
}

/** Whether a call's argument count fits its required arguments plus an
 *  optional trailing group. */
function arityOk(args: readonly ts.Expression[], groupIndex: number): boolean {
    return args.length === groupIndex || args.length === groupIndex + 1;
}

function toSlider(
    args: readonly ts.Expression[],
    sourceFile: ts.SourceFile,
): Pick<StaticSlider, 'max' | 'min' | 'unit' | 'value' | 'valueRange'> | null {
    if (!arityOk(args, GROUP_ARG_INDEX.$slider)) {
        return null;
    }
    const literals = args.slice(1, 4).map(sliderLiteral);
    if (literals.some((l) => l === null)) {
        return null;
    }
    try {
        const [value, min, max] = literals.map((l) =>
            parseSliderValue(l as number | string),
        );
        if (value.unit !== min.unit || value.unit !== max.unit) {
            return null;
        }
        if (min.volts >= max.volts) {
            return null;
        }
        return {
            max: max.volts,
            min: min.volts,
            unit: value.unit,
            value: value.volts,
            valueRange: rangeOf(args[1], sourceFile),
        };
    } catch {
        return null;
    }
}

function toButton(
    kind: '$btn' | '$toggleBtn',
    args: readonly ts.Expression[],
    sourceFile: ts.SourceFile,
): Pick<StaticButton, 'mode' | 'value' | 'stateRange'> | null {
    if (kind === '$btn') {
        return arityOk(args, GROUP_ARG_INDEX.$btn)
            ? { mode: 'gate', stateRange: null, value: false }
            : null;
    }
    const state = arityOk(args, GROUP_ARG_INDEX.$toggleBtn)
        ? booleanLiteral(args[1])
        : null;
    return state === null
        ? null
        : {
              mode: 'toggle',
              stateRange: rangeOf(args[1], sourceFile),
              value: state,
          };
}

/** Where a `$cGroup` call's collapsed state lives, or null when its params
 *  are not a literal the panel can edit. */
type CollapseTarget =
    /** No params object: one is appended. */
    | { kind: 'append' }
    /** A params object without `collapsed`: the property is added. */
    | { kind: 'insert'; params: ts.ObjectLiteralExpression }
    /** A `collapsed` boolean literal: it is replaced. */
    | { kind: 'replace'; literal: ts.Expression; state: boolean };

function collapseTarget(args: readonly ts.Expression[]): CollapseTarget | null {
    if (args.length === 1) {
        return { kind: 'append' };
    }
    const params = args[1];
    if (args.length !== 2 || !ts.isObjectLiteralExpression(params)) {
        return null;
    }
    const prop = params.properties.find(
        (p) => propertyName(p) === 'collapsed',
    );
    if (!prop) {
        return { kind: 'insert', params };
    }
    const state = ts.isPropertyAssignment(prop)
        ? booleanLiteral(prop.initializer)
        : null;
    return state === null
        ? null
        : {
              kind: 'replace',
              literal: (prop as ts.PropertyAssignment).initializer,
              state,
          };
}

function toGroupState(
    args: readonly ts.Expression[],
): Pick<StaticGroup, 'collapsed' | 'collapsible'> {
    const target = collapseTarget(args);
    return {
        collapsed: target?.kind === 'replace' ? target.state : false,
        collapsible: target !== null,
    };
}

function parse(source: string): ts.SourceFile {
    return ts.createSourceFile(
        'patch.js',
        source,
        ts.ScriptTarget.Latest,
        false,
        ts.ScriptKind.JS,
    );
}

/**
 * The edit that sets the collapsed state of the `$cGroup` call whose name
 * starts at `callStart`: the `collapsed` literal is replaced, the property
 * added to the params object, or a `{ collapsed }` object appended. Null
 * when no such group exists or its params cannot be edited.
 */
export function groupCollapseEdit(
    source: string,
    callStart: number,
    collapsed: boolean,
    layout: CodeStyle,
): { span: SourceRange; text: string } | null {
    const sourceFile = parse(source);
    const found = findControlCalls(sourceFile).find(
        (c) =>
            c.kind === '$cGroup' && c.name.getStart(sourceFile) === callStart,
    );
    const target = found && collapseTarget(found.call.arguments);
    if (!found || !target) {
        return null;
    }
    if (target.kind === 'replace') {
        return {
            span: rangeOf(target.literal, sourceFile),
            text: String(collapsed),
        };
    }
    const prop = `collapsed: ${String(collapsed)}`;
    const edit =
        target.kind === 'insert'
            ? insertObjectProperty(sourceFile, target.params, prop, layout)
            : appendObjectArgument(sourceFile, found.call, prop, layout);
    return { span: edit.span, text: edit.newText };
}

export function extractControls(source: string): StaticControls {
    const sourceFile = parse(source);
    const sliders: StaticControls['sliders'] = [];
    const buttons: StaticControls['buttons'] = [];
    const groups: StaticGroup[] = [];
    // Within a group, slider and button labels share one namespace; group
    // labels have their own.
    const controlKeys = new Set<string>();
    const groupKeys = new Set<string>();

    for (const { kind, call, name, group } of findControlCalls(sourceFile)) {
        const label = call.arguments[0];
        if (!label || !ts.isStringLiteral(label)) {
            continue;
        }
        const site: CallSite = {
            argsStart: call.arguments.pos,
            callStart: name.getStart(sourceFile),
        };
        const key = JSON.stringify([...group, label.text]);

        if (kind === '$cGroup') {
            if (!groupKeys.has(key)) {
                groupKeys.add(key);
                groups.push({
                    ...site,
                    ...toGroupState(call.arguments),
                    group,
                    label: label.text,
                });
            }
            continue;
        }

        if (controlKeys.has(key)) {
            continue;
        }
        controlKeys.add(key);
        const incomplete: IncompleteControl = {
            ...site,
            group,
            incomplete: true,
            label: label.text,
        };
        if (kind === '$slider') {
            const slider = toSlider(call.arguments, sourceFile);
            sliders.push(
                slider
                    ? { ...slider, ...site, group, label: label.text }
                    : incomplete,
            );
        } else {
            const button = toButton(kind, call.arguments, sourceFile);
            buttons.push(
                button
                    ? { ...button, ...site, group, label: label.text }
                    : incomplete,
            );
        }
    }

    return { buttons, groups, sliders };
}

export function isIncomplete<T>(
    c: T | IncompleteControl,
): c is IncompleteControl {
    return 'incomplete' in (c as object);
}

function sameGroup(a: string[], b: string[]): boolean {
    return a.length === b.length && a.every((label, i) => label === b[i]);
}

function resolveList<T extends CallSite & { label: string; group: string[] }>(
    current: Array<T | IncompleteControl>,
    previous: Array<T & { incomplete: boolean }>,
): Array<T & { incomplete: boolean }> {
    const resolved: Array<T & { incomplete: boolean }> = [];
    for (const control of current) {
        if (!isIncomplete(control)) {
            resolved.push({ ...control, incomplete: false });
            continue;
        }
        const last = previous.find(
            (p) =>
                p.label === control.label && sameGroup(p.group, control.group),
        );
        // A call with no earlier valid state has nothing to show yet.
        if (last) {
            resolved.push({
                ...last,
                argsStart: control.argsStart,
                callStart: control.callStart,
                incomplete: true,
            });
        }
    }
    return resolved;
}

/**
 * Resolve incomplete controls to their state in `previous` — the controls
 * last resolved for the same buffer — so a control does not vanish while one
 * of its arguments is being retyped.
 */
export function resolveControls(
    current: StaticControls,
    previous: ResolvedControls | undefined,
): ResolvedControls {
    return {
        buttons: resolveList(current.buttons, previous?.buttons ?? []),
        groups: current.groups,
        sliders: resolveList(current.sliders, previous?.sliders ?? []),
    };
}
