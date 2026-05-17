import type {
    Collection,
    CollectionWithRange,
    GraphBuilder,
    ModuleOutput,
    Signal,
} from '../graph';

/** Bundle of `$setTempo`/`$setOutputGain`/`$setTimeSignature`/`$setEndOfChainCb`. */
export function createSettings(builder: GraphBuilder) {
    const $setTempo = (tempo: number) => {
        builder.setTempo(tempo);
    };
    const $setOutputGain = (gain: Signal) => {
        builder.setOutputGain(gain);
    };
    const $setTimeSignature = (numerator: number, denominator: number) => {
        if (!Number.isInteger(numerator) || numerator < 1) {
            throw new Error(
                `$setTimeSignature: numerator must be a positive integer, got ${numerator}`,
            );
        }
        if (!Number.isInteger(denominator) || denominator < 1) {
            throw new Error(
                `$setTimeSignature: denominator must be a positive integer, got ${denominator}`,
            );
        }
        builder.setTimeSignature(numerator, denominator);
    };
    const $setEndOfChainCb = (
        cb: (
            mixed: Collection,
        ) => ModuleOutput | Collection | CollectionWithRange,
    ) => {
        builder.setEndOfChainCb(cb);
    };
    return { $setTempo, $setOutputGain, $setTimeSignature, $setEndOfChainCb };
}
