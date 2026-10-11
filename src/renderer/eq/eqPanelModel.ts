/**
 * The panel's view of an EQ's params, decoded from the `appliedPatch` params
 * object, and the encoders that turn panel values back into param values
 * (sent to the engine) and source literals (written into the code). Both
 * encoders emit the same quantized value, so after a plain gesture code and
 * audio agree exactly.
 */
import {
    DEFAULT_Q,
    type EqSectionType,
    EQ_SECTION_TYPES,
    constantSignalValue,
    hzToVoct,
    voctToHz,
} from './eqMath';

/** Raw params of one section as the engine receives them. */
export interface RawEqSection {
    type?: string;
    freq: unknown;
    gain?: unknown;
    q?: unknown;
}

export interface RawEqParams {
    input: unknown;
    /** One list shared by every channel, or one list per channel; optional
     *  in the code, always present once the panel holds the params. */
    sections: RawEqSection[] | RawEqSection[][];
    adaptiveQ?: boolean;
    ui?: boolean;
    [key: string]: unknown;
}

export interface EqSectionView {
    type: EqSectionType;
    /**
     * Current values. A signal-driven param shows the engine's live value,
     * or null until the first one arrives.
     */
    freqHz: number | null;
    gainDb: number | null;
    q: number | null;
    /** Signal-driven params, which the panel shows but cannot move. */
    driven: { freq: boolean; gain: boolean; q: boolean };
    /**
     * How the source spells freq: V/Oct numbers stay numbers, everything else
     * (hz strings, note names, absent) is written as an hz string.
     */
    freqStyle: 'hz' | 'volts';
}

export function isPerChannel(params: RawEqParams): boolean {
    return Array.isArray(params.sections[0]);
}

/**
 * The section lists, one per channel list; the shared form is a single list.
 * The lists are the params' own arrays, so mutating them edits the params.
 */
export function sectionLists(params: RawEqParams): RawEqSection[][] {
    return isPerChannel(params)
        ? (params.sections as RawEqSection[][])
        : [params.sections as RawEqSection[]];
}

function decodeType(value: unknown): EqSectionType {
    return EQ_SECTION_TYPES.includes(value as EqSectionType)
        ? (value as EqSectionType)
        : 'peak';
}

/** An absent optional param takes its engine default; a present one must be
 *  a constant to be editable. */
function decodeOptional(value: unknown, fallback: number): number | null {
    return value === undefined || value === null
        ? fallback
        : constantSignalValue(value);
}

export function decodeSection(raw: RawEqSection): EqSectionView {
    const volts = constantSignalValue(raw.freq);
    const gainDb = decodeOptional(raw.gain, 0);
    const q = decodeOptional(raw.q, DEFAULT_Q);
    return {
        driven: { freq: volts === null, gain: gainDb === null, q: q === null },
        freqHz: volts === null ? null : voctToHz(volts),
        freqStyle: typeof raw.freq === 'number' ? 'volts' : 'hz',
        gainDb,
        q,
        type: decodeType(raw.type),
    };
}

/** `[freq V/Oct, gain dB, q]` the engine reports a section running at. */
export type LiveSectionValues = [number, number, number];

/** Fill a section's signal-driven params from the engine's live values. */
export function withLive(
    sec: EqSectionView,
    live: LiveSectionValues | undefined,
): EqSectionView {
    if (!live || !sectionIsModulated(sec)) {
        return sec;
    }
    const finite = (v: number) => (Number.isFinite(v) ? v : null);
    return {
        ...sec,
        freqHz: sec.driven.freq ? finite(voctToHz(live[0])) : sec.freqHz,
        gainDb: sec.driven.gain ? finite(live[1]) : sec.gainDb,
        q: sec.driven.q ? finite(live[2]) : sec.q,
    };
}

/** True when any of the section's shaping params is signal-driven. */
export function sectionIsModulated(sec: EqSectionView): boolean {
    return sec.driven.freq || sec.driven.gain || sec.driven.q;
}

/** A param value in both of its forms. */
export interface EncodedValue {
    /** JSON value for the engine's params. */
    engine: string | number;
    /** Source literal, using `quote` for strings. */
    code: (quote: string) => string;
}

function formatNumber(v: number): string {
    return String(Number(v.toPrecision(6)));
}

export function encodeFreq(hz: number, style: 'hz' | 'volts'): EncodedValue {
    if (style === 'volts') {
        const volts = Number(hzToVoct(hz).toFixed(4));
        return { code: () => formatNumber(volts), engine: volts };
    }
    const text = `${formatNumber(hz)}hz`;
    return { code: (quote) => `${quote}${text}${quote}`, engine: text };
}

export function encodeNumber(v: number): EncodedValue {
    return { code: () => formatNumber(v), engine: v };
}

export function encodeType(type: EqSectionType): EncodedValue {
    return { code: (quote) => `${quote}${type}${quote}`, engine: type };
}

/** Panel-editable fields of a section. */
export type EqField = 'type' | 'freq' | 'gain' | 'q';

/** A panel change to one section, in panel units. */
export interface EqSectionChange {
    type?: EqSectionType;
    freqHz?: number;
    gainDb?: number;
    q?: number;
}

/** Encode a change into per-field values. */
export function encodeChange(
    change: EqSectionChange,
    freqStyle: 'hz' | 'volts',
): Partial<Record<EqField, EncodedValue>> {
    const out: Partial<Record<EqField, EncodedValue>> = {};
    if (change.type !== undefined) {
        out.type = encodeType(change.type);
    }
    if (change.freqHz !== undefined) {
        out.freq = encodeFreq(change.freqHz, freqStyle);
    }
    if (change.gainDb !== undefined) {
        out.gain = encodeNumber(change.gainDb);
    }
    if (change.q !== undefined) {
        out.q = encodeNumber(change.q);
    }
    return out;
}
