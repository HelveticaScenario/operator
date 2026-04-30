import type { BufferOutputRef, Collection, ModuleOutput } from '../graph';

type BufferFn = (
    input: ModuleOutput | Collection | number,
    lengthSeconds: number,
    config?: { id?: string },
) => BufferOutputRef;
type DeferredFn = (channels?: number) => {
    length: number;
    set(outputs: ModuleOutput | Iterable<ModuleOutput>): void;
};
type MixFn = (...args: unknown[]) => unknown;

/**
 * `$delay(input, feedbackCb, length)` — convenience wrapper for feedback delay
 * patterns. Defers, mixes input with the deferred signal, buffers the mix, and
 * resolves the deferred signal via `feedbackCb(buffer)`.
 */
export function create$delay(deps: {
    $buffer: BufferFn;
    $deferred: DeferredFn;
    $mix: MixFn;
}) {
    const { $buffer, $deferred, $mix } = deps;
    return (
        input: Collection | ModuleOutput,
        feedbackCb: (buffer: BufferOutputRef) => Collection | ModuleOutput,
        length: number,
    ): Collection & { buffer: BufferOutputRef } => {
        const def = $deferred('length' in input ? input.length : 1);
        const mixed = $mix([input, def]) as Collection;
        const buf = $buffer(mixed, length);
        def.set(feedbackCb(buf));
        return Object.assign(mixed, { buffer: buf });
    };
}
