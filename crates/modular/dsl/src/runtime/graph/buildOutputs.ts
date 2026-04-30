import type { Collection } from './collection';
import type { ModuleOutput } from './moduleOutput';
import type { FactoryFunction, OutGroup } from './types';
import { GAIN_CURVE_EXP } from './types';

/**
 * Build per-channel signal collections from registered out/outMono groups.
 *
 * Returns an array of channel collections (one per registered group), where
 * each collection holds the resolved per-channel signals (with leading silence
 * for `baseChannel` offsets). Caller mixes these into a single PolySignal.
 */
export function buildChannelCollections(
    outGroups: Map<number, OutGroup[]>,
    factories: {
        stereoMixer: FactoryFunction;
        scaleAndShift: FactoryFunction;
        curve: FactoryFunction;
    },
): (ModuleOutput | number)[][] {
    const allChannelCollections: (ModuleOutput | number)[][] = [];

    // Sort by baseChannel for deterministic processing
    const sortedChannels = [...outGroups.keys()].sort((a, b) => a - b);

    for (const baseChannel of sortedChannels) {
        const groups = outGroups.get(baseChannel)!;

        for (const group of groups) {
            let outputSignals: ModuleOutput[];

            if (group.type === 'stereo') {
                const stereoOut = factories.stereoMixer(group.outputs, {
                    pan: group.pan ?? 0,
                    width: group.width ?? 0,
                }) as Collection;

                if (group.gain !== undefined) {
                    const curvedAmp = factories.curve(
                        group.gain,
                        GAIN_CURVE_EXP,
                    );
                    const gained = factories.scaleAndShift(
                        [...stereoOut],
                        curvedAmp,
                    ) as Collection;
                    outputSignals = [...gained];
                } else {
                    outputSignals = [...stereoOut];
                }
            } else {
                // Mono: collapse stereo mix to first channel
                const mixOut = (
                    factories.stereoMixer(group.outputs, {
                        pan: -5,
                        width: 0,
                    }) as Collection
                )[0];

                let finalOut: ModuleOutput;
                if (group.gain !== undefined) {
                    const curvedAmp = factories.curve(
                        group.gain,
                        GAIN_CURVE_EXP,
                    );
                    finalOut = factories.scaleAndShift(
                        mixOut,
                        curvedAmp,
                    ) as ModuleOutput;
                } else {
                    finalOut = mixOut;
                }
                outputSignals = [finalOut];
            }

            const channelCollection: (ModuleOutput | number)[] = [];

            // Add silent channels for baseChannel offset (Signal::Volts(0.0))
            for (let i = 0; i < baseChannel; i++) {
                channelCollection.push(0);
            }

            channelCollection.push(...outputSignals);
            allChannelCollections.push(channelCollection);
        }
    }

    return allChannelCollections;
}
