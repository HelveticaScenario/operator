/**
 * Decide whether applying a patch update should restart the transport clock.
 *
 * The clock restarts only when playback switches from one buffer (song) to a
 * different one — i.e. the source identity differs from the previously-applied
 * source. Re-evaluating the same buffer (unchanged id) keeps the clock running
 * so live-coding never interrupts the transport. The very first apply (no
 * previous source) also keeps it: starting from a stopped engine already begins
 * the transport at zero on its own.
 *
 * `sourceId` is a stable per-buffer identity (the editor tab's id), not a file
 * path — so renaming or saving the playing buffer does not count as a switch.
 */
export function isBufferSwitch(
    previousSourceId: string | null,
    nextSourceId: string | null | undefined,
): boolean {
    return (
        Boolean(nextSourceId) &&
        previousSourceId !== null &&
        nextSourceId !== previousSourceId
    );
}
