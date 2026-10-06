/**
 * Volts/Hz/note conversions for unit-aware sliders.
 *
 * Pitch convention: 1V/oct, 0V = C4. Note strings follow the Rust
 * string-signal convention (crates/modular_core/src/types.rs): letter a-g,
 * optional #/b, optional octave defaulting to 4.
 *
 * Shared between the main-process executor and the renderer, so it must stay
 * free of Node.js and Electron dependencies.
 */

export type SliderUnit = 'number' | 'hz' | 'note';

const C4_HZ = 261.6255653005986;

const HZ_RE = /^(-?\d*\.?\d+)hz$/i;
// Mirrors the Rust RE_NOTE (crates/modular_core/src/types.rs): the letter is
// case-insensitive but the accidental is strictly lowercase `b` or `#`, so
// e.g. "AB3" is rejected rather than silently parsed without the flat.
const NOTE_RE = /^([a-gA-G])([#b]?)(-?\d+)?$/;

const SEMITONES: Record<string, number> = {
    a: 9,
    b: 11,
    c: 0,
    d: 2,
    e: 4,
    f: 5,
    g: 7,
};

const SHARP_NAMES = [
    'c',
    'c#',
    'd',
    'd#',
    'e',
    'f',
    'f#',
    'g',
    'g#',
    'a',
    'a#',
    'b',
];

export function hzToVolts(hz: number): number {
    return Math.log2(hz / C4_HZ);
}

export function voltsToHz(volts: number): number {
    return C4_HZ * 2 ** volts;
}

/**
 * Parse a slider value/min/max argument into its unit and V/Oct volts.
 * Accepts a finite number (unit 'number', passed through as volts), an hz
 * string like "440hz", or a note string like "c4" / "a#3".
 */
export function parseSliderValue(v: number | string): {
    unit: SliderUnit;
    volts: number;
} {
    if (typeof v === 'number') {
        if (!isFinite(v)) {
            throw new Error('slider values must be finite numbers');
        }
        return { unit: 'number', volts: v };
    }
    if (typeof v !== 'string') {
        throw new Error(
            'slider values must be numbers, hz strings, or note strings',
        );
    }
    const hzMatch = v.match(HZ_RE);
    if (hzMatch) {
        const hz = parseFloat(hzMatch[1]);
        if (!isFinite(hz) || hz <= 0) {
            throw new Error(`slider hz value must be positive: "${v}"`);
        }
        return { unit: 'hz', volts: hzToVolts(hz) };
    }
    const noteMatch = v.match(NOTE_RE);
    if (noteMatch) {
        const semitone =
            SEMITONES[noteMatch[1].toLowerCase()] +
            (noteMatch[2] === '#' ? 1 : noteMatch[2] === 'b' ? -1 : 0);
        const octave =
            noteMatch[3] !== undefined ? parseInt(noteMatch[3], 10) : 4;
        const midi = (octave + 1) * 12 + semitone;
        return { unit: 'note', volts: (midi - 60) / 12 };
    }
    throw new Error(
        `invalid slider value "${v}" — expected a number, an hz string like "440hz", or a note like "c4"`,
    );
}

/** Snap V/Oct volts to the nearest semitone (exact MIDI-integer volts). */
export function snapVoltsToSemitone(volts: number): number {
    return (Math.round(volts * 12 + 60) - 60) / 12;
}

/**
 * Nearest note name for V/Oct volts, sharp spellings, e.g. "a#4". Uses the
 * same midi rounding as snapVoltsToSemitone, so parsing the returned name
 * yields exactly the snapped volts.
 */
export function voltsToNoteName(volts: number): string {
    const midi = Math.round(volts * 12 + 60);
    const name = SHARP_NAMES[((midi % 12) + 12) % 12];
    const octave = Math.floor(midi / 12) - 1;
    return `${name}${octave}`;
}

/**
 * Format an hz-string literal for source writeback. The hz signal grammar
 * rejects exponent notation, so exponent output is expanded to plain decimal
 * digits — preserving all significant digits, so the literal re-parses to a
 * positive frequency at any magnitude. Slider hz values are strictly positive.
 */
export function formatHzLiteral(hz: number): string {
    const s = Number(hz.toPrecision(6)).toString();
    return `${/[eE]/.test(s) ? expandExponent(s) : s}hz`;
}

/** Expand a positive number's exponent-notation string (e.g. "2.5e-7") into
 *  plain decimal digits ("0.00000025") without losing significant digits. */
function expandExponent(s: string): string {
    const [mantissa, expPart] = s.split(/[eE]/);
    const exp = parseInt(expPart, 10);
    const dot = mantissa.indexOf('.');
    const digits = mantissa.replace('.', '');
    const pointPos = (dot === -1 ? mantissa.length : dot) + exp;
    if (pointPos <= 0) {
        return `0.${'0'.repeat(-pointPos)}${digits}`;
    }
    if (pointPos >= digits.length) {
        return digits + '0'.repeat(pointPos - digits.length);
    }
    return `${digits.slice(0, pointPos)}.${digits.slice(pointPos)}`;
}
