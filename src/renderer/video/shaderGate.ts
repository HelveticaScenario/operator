/** What to do with a shader built for a patch update, given what the engine has done. */
export type GateVerdict = 'wait' | 'activate' | 'drop';

/**
 * A shader belongs to the audio patch it was compiled with: the audio taps it
 * reads are numbered by that patch. It goes live once the engine has applied
 * that update (or a later one), and is dropped if the engine discarded the
 * update, which leaves the previous patch and shader running.
 */
export function gateVerdict(
    updateId: number,
    applied: number,
    cancelled: number,
): GateVerdict {
    if (applied >= updateId) return 'activate';
    if (cancelled >= updateId) return 'drop';
    return 'wait';
}
