import type { ProcessedModuleSchema } from './paramsSchema';

export const SIGNAL_GROUP_KIND = 'SignalGroup';

/** Rust's PORT_MAX_CHANNELS — the engine rejects wider signal arrays. */
const MAX_CHANNELS = 64;

/**
 * A param value tagged into a cartesian signal group. Constructed only by
 * $g1/$g2/$g3; consumed and erased by expandSignalGroups before the
 * PatchGraph is emitted — the Rust engine never sees this shape.
 */
export interface SignalGroup {
    __kind: typeof SIGNAL_GROUP_KIND;
    group: 1 | 2 | 3;
    signals: unknown;
}

export function isSignalGroupLike(value: unknown): value is SignalGroup {
    return (
        typeof value === 'object' &&
        value !== null &&
        !Array.isArray(value) &&
        (value as { __kind?: unknown }).__kind === SIGNAL_GROUP_KIND
    );
}

function makeGroupTagger(group: 1 | 2 | 3) {
    return (signals: unknown): SignalGroup => {
        if (isSignalGroupLike(signals)) {
            throw new Error(
                `$g${group}(...) cannot wrap another signal group — nesting is not allowed`,
            );
        }
        return { __kind: SIGNAL_GROUP_KIND, group, signals };
    };
}

/**
 * Tag a param value into signal group 1/2/3. Params in different groups
 * multiply (cartesian product of channels) within a module instead of cycling
 * together; params in the same group cycle as usual, and untagged params are
 * group 0. Lower-numbered groups vary fastest across the resulting channels.
 */
export const $g1 = makeGroupTagger(1);
export const $g2 = makeGroupTagger(2);
export const $g3 = makeGroupTagger(3);

/**
 * Reject SignalGroup wrappers anywhere below the root of a param value — a
 * group must tag the entire param, never an element of it. Walking pattern
 * payloads is safe: parser-generated ASTs can never contain a SignalGroup
 * (only the $gN taggers construct one).
 */
export function assertSignalGroupsTopLevelOnly(value: unknown): void {
    const walkBelowRoot = (v: unknown): void => {
        if (isSignalGroupLike(v)) {
            throw new Error(
                '$g1/$g2/$g3 must wrap the entire value passed to a parameter — a signal group cannot appear inside an array or object',
            );
        }
        walkChildren(v);
    };
    const walkChildren = (v: unknown): void => {
        if (Array.isArray(v)) {
            for (const item of v) {
                walkBelowRoot(item);
            }
        } else if (typeof v === 'object' && v !== null) {
            for (const child of Object.values(v)) {
                walkBelowRoot(child);
            }
        }
    };
    // The root itself may be a wrapper; only its contents are restricted.
    walkChildren(isSignalGroupLike(value) ? value.signals : value);
}

interface GroupMember {
    name: string;
    group: number;
    /** Unwrapped signals value (scalar or array, post-replaceSignals). */
    signals: unknown;
    /** Channel count: array length, or 1 for a scalar/cable. */
    n: number;
    wrapped: boolean;
}

/**
 * Resolve $gN wrappers on a module's params into plain full-length signal
 * arrays implementing the cartesian product: the widest emitted array sets
 * the module's channel count, and the engine's per-channel modulo read
 * becomes the identity.
 *
 * Returns `params` untouched (same reference) when no param is wrapped.
 * Never mutates `params`.
 */
export function expandSignalGroups(
    params: Record<string, unknown>,
    schema: ProcessedModuleSchema,
    moduleType: string,
): Record<string, unknown> {
    const wrappedNames = Object.keys(params).filter((k) =>
        isSignalGroupLike(params[k]),
    );
    if (wrappedNames.length === 0) {
        return params;
    }

    // Wrappers are only valid on true polyphonic signal params. Mono
    // (summing) inputs must reject them: expanding a summed array would
    // multiply its value, and a group tag on one can't mean anything.
    for (const name of wrappedNames) {
        const wrapper = params[name] as SignalGroup;
        const desc = schema.paramsByName?.[name];
        if (!desc?.isPolySignalInput) {
            const why = desc?.isMonoSignalInput
                ? 'is a mono (summing) input'
                : 'is not a polyphonic signal input';
            throw new Error(
                `$g${wrapper.group}: parameter "${name}" of ${moduleType} ${why} — signal groups are not supported here`,
            );
        }
    }

    // Every set polyphonic signal param participates, untagged ones as
    // group 0.
    const members: GroupMember[] = [];
    for (const desc of schema.params) {
        if (!desc.isPolySignalInput) {
            continue;
        }
        const raw = params[desc.name];
        if (raw === undefined || raw === null) {
            continue;
        }
        const wrapped = isSignalGroupLike(raw);
        const signals = wrapped ? raw.signals : raw;
        const n = Array.isArray(signals) ? signals.length : 1;
        if (n === 0) {
            // The engine rejects empty signal arrays and will produce an error itself.
            continue;
        }
        members.push({
            name: desc.name,
            group: wrapped ? raw.group : 0,
            signals,
            n,
            wrapped,
        });
    }

    const widths = [0, 0, 0, 0];
    for (const m of members) {
        widths[m.group] = Math.max(widths[m.group], m.n);
    }

    const total = widths.reduce((acc, w) => acc * Math.max(w, 1), 1);
    if (total > MAX_CHANNELS) {
        const detail = widths
            .map((w, g) => (w > 0 ? `g${g}:${w}` : null))
            .filter(Boolean)
            .join(' x ');
        throw new Error(
            `signal groups on ${moduleType} multiply to ${total} channels (${detail}); the limit is ${MAX_CHANNELS}`,
        );
    }

    // Ascending group order. lower-numbered group varies fastest across
    // channels.
    const strides = [1, 1, 1, 1];
    let acc = 1;
    for (let g = 0; g <= 3; g++) {
        strides[g] = acc;
        if (widths[g] > 0) {
            acc *= widths[g];
        }
    }

    const out: Record<string, unknown> = { ...params };
    for (const m of members) {
        if (m.n === 1) {
            if (m.wrapped) {
                out[m.name] = m.signals;
            }
            continue;
        }
        const arr = m.signals as unknown[];
        const stride = strides[m.group];
        const width = widths[m.group];
        out[m.name] = Array.from(
            { length: total },
            // The inner % width keeps a narrower member cycling inside its
            // own group's slots.
            (_, ch) => arr[(Math.floor(ch / stride) % width) % m.n],
        );
    }
    return out;
}
