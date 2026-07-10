/**
 * Utility for editing the `mute` / `solo` properties of `.out(...)` and
 * `.outMono(...)` calls in DSL source code.
 *
 * The VU meter panel's M/S buttons anchor each out call with a Monaco tracked
 * decoration; this module turns that anchor (a character offset at the method
 * name) plus the desired property change into a single text edit. It uses the
 * same lightweight, string/comment-aware scanning as `sliderSourceEdit` — no
 * ts-morph, so it can run in the renderer process.
 */

import { findIgnoredRanges } from './sliderSourceEdit';

export type OutOptionProp = 'mute' | 'solo';

export interface OutOptionEdit {
    /** Inclusive start character offset */
    start: number;
    /** Exclusive end character offset */
    end: number;
    /** Replacement text */
    text: string;
}

/** A top-level argument's trimmed span within the call's parens. */
interface ArgSpan {
    start: number;
    end: number;
}

/** True if `offset` falls within any ignored range. */
function isIgnored(
    offset: number,
    ranges: Array<[number, number]>,
): boolean {
    for (const [start, end] of ranges) {
        if (offset >= start && offset < end) {
            return true;
        }
    }
    return false;
}

/**
 * Find the offset of the closing bracket matching the opener at `open`,
 * respecting nesting and skipping string/comment contents. Returns -1 when
 * unbalanced.
 */
function findMatching(
    source: string,
    open: number,
    ignored: Array<[number, number]>,
): number {
    const openCh = source[open];
    const closeCh = openCh === '(' ? ')' : openCh === '{' ? '}' : ']';
    let depth = 0;
    for (let i = open; i < source.length; i++) {
        if (isIgnored(i, ignored)) {
            continue;
        }
        const c = source[i];
        if (c === '(' || c === '{' || c === '[') {
            depth++;
        } else if (c === ')' || c === '}' || c === ']') {
            depth--;
            if (depth === 0) {
                return c === closeCh ? i : -1;
            }
        }
    }
    return -1;
}

/** Split the span (open, close) into top-level comma-separated trimmed args. */
function splitArgs(
    source: string,
    open: number,
    close: number,
    ignored: Array<[number, number]>,
): ArgSpan[] {
    const args: ArgSpan[] = [];
    let depth = 0;
    let argStart = open + 1;
    const pushArg = (rawEnd: number) => {
        let s = argStart;
        let e = rawEnd;
        while (s < e && /\s/.test(source[s])) {
            s++;
        }
        while (e > s && /\s/.test(source[e - 1])) {
            e--;
        }
        if (e > s) {
            args.push({ end: e, start: s });
        }
    };
    for (let i = open + 1; i < close; i++) {
        if (isIgnored(i, ignored)) {
            continue;
        }
        const c = source[i];
        if (c === '(' || c === '{' || c === '[') {
            depth++;
        } else if (c === ')' || c === '}' || c === ']') {
            depth--;
        } else if (c === ',' && depth === 0) {
            pushArg(i);
            argStart = i + 1;
        }
    }
    pushArg(close);
    return args;
}

/** A `prop: <value>` property found at depth 1 inside an object literal. */
interface PropSpan {
    /** Offset of the property key */
    keyStart: number;
    /** Trimmed span of the value expression */
    valueStart: number;
    valueEnd: number;
}

/**
 * Find the span of a bare-identifier property named `prop` at depth 1 inside
 * the object literal spanning [open, close].
 */
function findProperty(
    source: string,
    open: number,
    close: number,
    prop: string,
    ignored: Array<[number, number]>,
): PropSpan | null {
    let depth = 0;
    for (let i = open; i <= close; i++) {
        if (isIgnored(i, ignored)) {
            continue;
        }
        const c = source[i];
        if (c === '(' || c === '{' || c === '[') {
            depth++;
            continue;
        }
        if (c === ')' || c === '}' || c === ']') {
            depth--;
            continue;
        }
        if (depth !== 1 || !source.startsWith(prop, i)) {
            continue;
        }
        // Must be a whole identifier: not preceded or followed by word chars.
        const before = i > 0 ? source[i - 1] : '';
        const afterIdx = i + prop.length;
        if (/[\w$]/.test(before) || /[\w$]/.test(source[afterIdx] ?? '')) {
            continue;
        }
        // Skip whitespace to the `:`.
        let j = afterIdx;
        while (j < close && /\s/.test(source[j])) {
            j++;
        }
        if (source[j] !== ':') {
            continue;
        }
        j++;
        while (j < close && /\s/.test(source[j])) {
            j++;
        }
        // Value runs to the next depth-1 comma or the closing brace.
        let valueEnd = -1;
        let d = 0;
        for (let k = j; k < close; k++) {
            if (isIgnored(k, ignored)) {
                continue;
            }
            const ck = source[k];
            if (ck === '(' || ck === '{' || ck === '[') {
                d++;
            } else if (ck === ')' || ck === '}' || ck === ']') {
                d--;
            } else if (ck === ',' && d === 0) {
                valueEnd = k;
                break;
            }
        }
        if (valueEnd === -1) {
            valueEnd = close;
        }
        while (valueEnd > j && /\s/.test(source[valueEnd - 1])) {
            valueEnd--;
        }
        return { keyStart: i, valueEnd, valueStart: j };
    }
    return null;
}

/** True when the object literal spanning [open, close] has no properties. */
function objectIsEmpty(
    source: string,
    open: number,
    close: number,
    ignored: Array<[number, number]>,
): boolean {
    for (let i = open + 1; i < close; i++) {
        if (isIgnored(i, ignored)) {
            continue;
        }
        if (!/\s/.test(source[i])) {
            return false;
        }
    }
    return true;
}

const BOOLEAN_LITERAL_RE = /^(true|false)$/;
const NUMERIC_LITERAL_RE = /^-?(\d+\.?\d*|\.\d+)$/;

/**
 * Compute the text edit that sets (`value` true) or removes (`value` false)
 * the `prop` property in the options argument of the `.out(...)` /
 * `.outMono(...)` call whose method name starts at `anchorOffset`.
 *
 * Returns null when the call can't be parsed or the existing property value
 * is not a `true`/`false` literal — callers skip the source edit but still
 * apply the live graph update (the same silent-skip contract as sliders).
 */
export function computeOutOptionEdit(
    source: string,
    anchorOffset: number,
    prop: OutOptionProp,
    value: boolean,
): OutOptionEdit | null {
    return computeOptionPropertyEdit(
        source,
        anchorOffset,
        prop,
        value ? 'true' : null,
        BOOLEAN_LITERAL_RE,
    );
}

/**
 * Same contract as `computeOutOptionEdit` for numeric-valued options (the
 * pan knob): a number sets `prop: <value>`, null removes the property. An
 * existing value must be a numeric literal or the edit is skipped.
 */
export function computeOutNumericOptionEdit(
    source: string,
    anchorOffset: number,
    prop: string,
    value: number | null,
): OutOptionEdit | null {
    return computeOptionPropertyEdit(
        source,
        anchorOffset,
        prop,
        value === null ? null : String(Number(value.toPrecision(4))),
        NUMERIC_LITERAL_RE,
    );
}

/**
 * Compute the text edit that sets the master output gain: updates the
 * numeric literal in an existing `$setOutputGain(...)` call, or appends a
 * new call at the end of the source. Returns null when the existing call's
 * argument is not a numeric literal (signal-driven — the fader is locked).
 */
export function computeSetOutputGainEdit(
    source: string,
    value: number,
): OutOptionEdit | null {
    const newText = String(Number(value.toPrecision(4)));
    const ignored = findIgnoredRanges(source);
    const pattern = /\$setOutputGain\s*\(/g;
    let match: RegExpExecArray | null;
    while ((match = pattern.exec(source)) !== null) {
        if (isIgnored(match.index, ignored)) {
            continue;
        }
        const open = match.index + match[0].length - 1;
        const close = findMatching(source, open, ignored);
        if (close === -1) {
            return null;
        }
        let start = open + 1;
        while (start < close && /\s/.test(source[start])) {
            start++;
        }
        let end = close;
        while (end > start && /\s/.test(source[end - 1])) {
            end--;
        }
        if (!NUMERIC_LITERAL_RE.test(source.slice(start, end))) {
            return null;
        }
        return { end, start, text: newText };
    }
    // No call yet — append one on its own line at the end.
    const needsNewline = source.length > 0 && !source.endsWith('\n');
    return {
        end: source.length,
        start: source.length,
        text: `${needsNewline ? '\n' : ''}$setOutputGain(${newText})\n`,
    };
}

function computeOptionPropertyEdit(
    source: string,
    anchorOffset: number,
    prop: string,
    newText: string | null,
    literalRe: RegExp,
): OutOptionEdit | null {
    const ignored = findIgnoredRanges(source);
    if (isIgnored(anchorOffset, ignored)) {
        return null;
    }

    const nameMatch = /^(outMono|out)\b/.exec(source.slice(anchorOffset));
    if (!nameMatch) {
        return null;
    }
    const method = nameMatch[1];

    let open = anchorOffset + method.length;
    while (open < source.length && /\s/.test(source[open])) {
        open++;
    }
    if (source[open] !== '(') {
        return null;
    }
    const close = findMatching(source, open, ignored);
    if (close === -1) {
        return null;
    }

    const args = splitArgs(source, open, close, ignored);
    // `.out(...)` carries its options object as the sole argument. `.outMono`
    // normally takes a leading positional channel, so its options object is
    // the second argument — unless the single-object overload `.outMono({…})`
    // placed it first (an object as the first argument).
    const firstIsObject =
        args[0] !== undefined && source[args[0].start] === '{';
    const optionsIndex = method === 'out' || firstIsObject ? 0 : 1;
    const optionsArg = args[optionsIndex] as ArgSpan | undefined;
    const optionsIsObject =
        optionsArg !== undefined && source[optionsArg.start] === '{';

    if (newText !== null) {
        // --- Set `prop: <newText>` ---
        if (!optionsIsObject) {
            if (method === 'out') {
                // `.out()` — arg list must be empty (a non-object first arg
                // is not a valid out() call).
                if (optionsArg !== undefined) {
                    return null;
                }
                return {
                    end: close,
                    start: open + 1,
                    text: `{ ${prop}: ${newText} }`,
                };
            }
            // `.outMono(...)` — wrap a positional gain, append an options
            // object, or supply the default channel for a bare call. Setting
            // `gain` itself keeps the positional form.
            if (optionsArg !== undefined) {
                const gainText = source.slice(
                    optionsArg.start,
                    optionsArg.end,
                );
                if (prop === 'gain') {
                    if (!literalRe.test(gainText)) {
                        return null;
                    }
                    return {
                        end: optionsArg.end,
                        start: optionsArg.start,
                        text: newText,
                    };
                }
                return {
                    end: optionsArg.end,
                    start: optionsArg.start,
                    text: `{ gain: ${gainText}, ${prop}: ${newText} }`,
                };
            }
            if (args.length === 1) {
                return {
                    end: args[0].end,
                    start: args[0].end,
                    text:
                        prop === 'gain'
                            ? `, ${newText}`
                            : `, { ${prop}: ${newText} }`,
                };
            }
            return {
                end: close,
                start: open + 1,
                text:
                    prop === 'gain'
                        ? `0, ${newText}`
                        : `0, { ${prop}: ${newText} }`,
            };
        }

        const objOpen = optionsArg.start;
        const objClose = optionsArg.end - 1;
        if (source[objClose] !== '}') {
            return null;
        }
        const existing = findProperty(source, objOpen, objClose, prop, ignored);
        if (existing) {
            const valueText = source.slice(
                existing.valueStart,
                existing.valueEnd,
            );
            if (!literalRe.test(valueText)) {
                return null;
            }
            return {
                end: existing.valueEnd,
                start: existing.valueStart,
                text: newText,
            };
        }
        if (objectIsEmpty(source, objOpen, objClose, ignored)) {
            return {
                end: objClose + 1,
                start: objOpen,
                text: `{ ${prop}: ${newText} }`,
            };
        }
        // Insert before the closing brace, after any trailing comma.
        let insertAt = objClose;
        while (insertAt > objOpen + 1 && /\s/.test(source[insertAt - 1])) {
            insertAt--;
        }
        const needsComma = source[insertAt - 1] !== ',';
        return {
            end: objClose,
            start: insertAt,
            text: `${needsComma ? ',' : ''} ${prop}: ${newText} `,
        };
    }

    // --- Remove the property ---
    if (!optionsIsObject) {
        // A positional outMono gain literal is removable in place.
        if (
            method === 'outMono' &&
            prop === 'gain' &&
            optionsArg !== undefined
        ) {
            const gainText = source.slice(optionsArg.start, optionsArg.end);
            if (!literalRe.test(gainText)) {
                return null;
            }
            let argStart = optionsArg.start;
            while (argStart > open + 1 && /\s/.test(source[argStart - 1])) {
                argStart--;
            }
            if (source[argStart - 1] === ',') {
                argStart--;
            }
            return { end: optionsArg.end, start: argStart, text: '' };
        }
        return null;
    }
    const objOpen = optionsArg.start;
    const objClose = optionsArg.end - 1;
    if (source[objClose] !== '}') {
        return null;
    }
    const existing = findProperty(source, objOpen, objClose, prop, ignored);
    if (!existing) {
        return null;
    }
    const valueText = source.slice(existing.valueStart, existing.valueEnd);
    if (!literalRe.test(valueText)) {
        return null;
    }

    // Property removal span: through the following depth-1 comma if present,
    // otherwise back through the preceding comma.
    let removeStart = existing.keyStart;
    let removeEnd = existing.valueEnd;
    let k = removeEnd;
    while (k < objClose && /\s/.test(source[k])) {
        k++;
    }
    if (source[k] === ',') {
        removeEnd = k + 1;
        while (removeEnd < objClose && /\s/.test(source[removeEnd])) {
            removeEnd++;
        }
    } else {
        let p = removeStart;
        while (p > objOpen + 1 && /\s/.test(source[p - 1])) {
            p--;
        }
        if (source[p - 1] === ',') {
            removeStart = p - 1;
        }
    }

    // If this was the only property, drop the whole options argument.
    const remainder =
        source.slice(objOpen + 1, removeStart) +
        source.slice(removeEnd, objClose);
    if (/^\s*$/.test(remainder)) {
        if (method === 'out') {
            return { end: close, start: open + 1, text: '' };
        }
        // `.outMono(ch, { prop: true })` — remove the second arg and the
        // comma separating it from the channel.
        let argStart = optionsArg.start;
        while (argStart > open + 1 && /\s/.test(source[argStart - 1])) {
            argStart--;
        }
        if (source[argStart - 1] === ',') {
            argStart--;
        }
        return { end: optionsArg.end, start: argStart, text: '' };
    }

    return { end: removeEnd, start: removeStart, text: '' };
}
