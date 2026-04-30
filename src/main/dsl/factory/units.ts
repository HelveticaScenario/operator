/**
 * Unit conversion helpers exposed in the DSL as `hz()`, `note()`, `bpm()`.
 */

/** C4 = 261.6255653005986 Hz (440 / 2^(9/12)) */
const C4_HZ = 261.6255653005986;

/**
 * Convert Hz to V/oct.
 * V/oct = log2(Hz / C4) where 0V = C4 = MIDI 60.
 */
export function hz(frequency: number): number {
    if (frequency <= 0) {
        throw new Error('Frequency must be positive');
    }
    return Math.log2(frequency / C4_HZ);
}

/**
 * Note name to V/oct conversion.
 * Supports notes like "c4", "c#4", "db4", etc.
 */
export function note(noteName: string): number {
    const noteRegex = /^([a-g])([#b]?)(-?\d+)?$/i;
    const match = noteName.toLowerCase().match(noteRegex);

    if (!match) {
        throw new Error(`Invalid note name: ${noteName}`);
    }

    const [, noteLetter, accidental, octaveStr] = match;
    const octave = octaveStr ? parseInt(octaveStr, 10) : 3;

    const noteMap: Record<string, number> = {
        a: 9,
        b: 11,
        c: 0,
        d: 2,
        e: 4,
        f: 5,
        g: 7,
    };

    let semitone = noteMap[noteLetter];

    if (accidental === '#') {
        semitone += 1;
    } else if (accidental === 'b') {
        semitone -= 1;
    }

    const semitonesFromC4 = (octave - 4) * 12 + semitone;
    const frequency = C4_HZ * 2 ** (semitonesFromC4 / 12);

    return hz(frequency);
}

/**
 * Convert BPM (beats per minute) to V/oct frequency.
 * At 120 BPM that's 2 beats per second = 2 Hz.
 */
export function bpm(beatsPerMinute: number): number {
    if (beatsPerMinute <= 0) {
        throw new Error('BPM must be positive');
    }
    const frequency = beatsPerMinute / 60;
    return hz(frequency);
}
