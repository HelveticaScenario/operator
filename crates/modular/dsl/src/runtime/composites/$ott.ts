import type { Collection, ModuleOutput, PolySignal, Signal } from '../graph';
import type { NamespaceTree } from '../factory';
import { hz } from '../factory/units';

interface OttConfig {
    /**
     * Optional side-chain detector signal. When connected, the same
     * crossover network splits the sidechain into low/mid/high and each
     * band's compressor keys off the matching sidechain band — the gain is
     * still applied to `input`. Enables band-aware ducking (e.g. only
     * sidechain the low band against a kick).
     */
    sidechain?: PolySignal;
    /** wet/dry blend, 0–5 (default 5 = fully wet) */
    depth?: PolySignal;
    /** low/mid crossover (V/Oct, default ~120 Hz) */
    lowMidFreq?: PolySignal;
    /** mid/high crossover (V/Oct, default ~2500 Hz) */
    midHighFreq?: PolySignal;
    /** downward threshold in volts (default 1.0) */
    threshold?: PolySignal;
    /** downward ratio (default 4) */
    ratio?: PolySignal;
    /** upward threshold in volts (default 0.5) */
    upwardThreshold?: PolySignal;
    /** upward ratio (default 4) */
    upwardRatio?: PolySignal;
    /** envelope attack (seconds, default 0.003) */
    attack?: PolySignal;
    /** envelope release (seconds, default 0.05) */
    release?: PolySignal;
    /** per-band makeup as dB-voltage (-5V = -24dB, 0V = unity, +5V = +24dB, default 1V ≈ +4.8dB) */
    makeup?: PolySignal;
    /** per-band trim (V, 5 = unity, default 5) */
    lowGain?: PolySignal;
    midGain?: PolySignal;
    highGain?: PolySignal;
    id?: string;
}

type MixFactory = (...args: unknown[]) => unknown;
type FactoryFn = (...args: unknown[]) => unknown;

/**
 * `$ott` — three-band upward + downward compressor in the style of Xfer's
 * OTT. Splits the input into low/mid/high via `$xover`, applies aggressive
 * upward + downward compression to each band with `$comp`, sums the bands,
 * and crossfades against the original input via `depth`.
 *
 * Per-band trim (`lowGain` / `midGain` / `highGain`) uses `$scaleAndShift`
 * convention: 5 V = unity, 0 V = silence, 10 V = +6 dB.
 */
export function create$ott(deps: {
    namespaceTree: NamespaceTree;
    $mix: MixFactory;
}) {
    const { namespaceTree, $mix } = deps;
    const $xover = namespaceTree['$xover'];
    const $comp = namespaceTree['$comp'];
    const $scaleAndShift = namespaceTree['$scaleAndShift'];
    if (
        typeof $xover !== 'function' ||
        typeof $comp !== 'function' ||
        typeof $scaleAndShift !== 'function'
    ) {
        throw new Error(
            'DSL execution error: "$ott" requires "$xover", "$comp", and "$scaleAndShift" modules',
        );
    }

    const xover = $xover as FactoryFn;
    const comp = $comp as FactoryFn;
    const scaleAndShift = $scaleAndShift as FactoryFn;

    return (input: PolySignal, config: OttConfig = {}): Collection => {
        const compConf = {
            attack: config.attack ?? 0.003,
            makeup: config.makeup ?? 1.0,
            ratio: config.ratio ?? 4,
            release: config.release ?? 0.05,
            threshold: config.threshold ?? 1.0,
            upwardRatio: config.upwardRatio ?? 4,
            upwardThreshold: config.upwardThreshold ?? 0.5,
        };

        const lowMidFreq = config.lowMidFreq ?? hz(120);
        const midHighFreq = config.midHighFreq ?? hz(2500);
        const xoverConf = { lowMidFreq, midHighFreq };

        const bands = xover(input, xoverConf) as Collection & {
            low: Collection;
            mid: Collection;
            high: Collection;
        };

        // Split the side-chain through an identical crossover so each band's
        // compressor keys off the matching frequency range of the sidechain.
        const scBands =
            config.sidechain !== undefined
                ? (xover(config.sidechain, xoverConf) as Collection & {
                      low: Collection;
                      mid: Collection;
                      high: Collection;
                  })
                : null;

        const lowComp = comp(bands.low, {
            ...compConf,
            ...(scBands !== null && { sidechain: scBands.low }),
        });
        const midComp = comp(bands.mid, {
            ...compConf,
            ...(scBands !== null && { sidechain: scBands.mid }),
        });
        const highComp = comp(bands.high, {
            ...compConf,
            ...(scBands !== null && { sidechain: scBands.high }),
        });

        // Per-band trim — $scaleAndShift convention: scale=5 → unity gain.
        const low = scaleAndShift(lowComp, config.lowGain ?? 5, 0);
        const mid = scaleAndShift(midComp, config.midGain ?? 5, 0);
        const high = scaleAndShift(highComp, config.highGain ?? 5, 0);

        const wet = $mix([low, mid, high]) as Collection;

        // Crossfade dry vs wet using `depth` (0–5 V):
        //   dryWeight = 5 − depth → unity when depth=0, silence when depth=5
        //   wetWeight = depth      → silence when depth=0, unity when depth=5
        const depth = config.depth ?? 5;
        const dryWeight = scaleAndShift(depth, -5, 5);

        return $mix([
            scaleAndShift(input as ModuleOutput, dryWeight as Signal, 0),
            scaleAndShift(wet, depth, 0),
        ]) as Collection;
    };
}
