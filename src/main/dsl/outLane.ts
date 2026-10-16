import { captureSourceLocation } from './captureSourceLocation';
import {
    Collection,
    DeferredCollection,
    DeferredModuleOutput,
    type GraphBuilder,
    type ModuleOutput,
    type ModuleOutputWithRange,
} from './GraphBuilder';

/** A lane control's tap, with the range its source declares. */
export interface OutLaneControl {
    output: ModuleOutput;
    min: ModuleOutput;
    max: ModuleOutput;
}

/** A labeled out lane's signals, as compiled in toPatch. */
export interface OutLaneSources {
    /** Post-gain, post-mute audio: two channels for stereo outs, one for mono. */
    audio: ModuleOutput[];
    /** Mute/solo gate _smoothControl (5 audible, 0 silenced). */
    active: ModuleOutput;
    level: OutLaneControl;
    /** Center (0) for mono outs. */
    pan: OutLaneControl;
}

interface OutLaneControls {
    active: ModuleOutputWithRange;
    level: ModuleOutputWithRange;
    pan: ModuleOutputWithRange;
}

/**
 * `$outLane` result: a stereo Collection of the lane's audio, with its
 * mute/solo gate, gain, and pan as ranged properties. Slices keep the
 * properties.
 */
export class OutLane extends Collection {
    /** 5 V while the lane is audible, 0 V while muted or soloed out. */
    readonly active: ModuleOutputWithRange;
    /** The lane's gain in DSL units (5 = unity). */
    readonly level: ModuleOutputWithRange;
    /** The lane's pan (-5 left … +5 right). */
    readonly pan: ModuleOutputWithRange;

    constructor(audio: ModuleOutput[], controls: OutLaneControls) {
        super(...audio);
        this.active = controls.active;
        this.level = controls.level;
        this.pan = controls.pan;
    }

    override slice(start?: number, end?: number): this {
        return new OutLane(this.items.slice(start, end), this) as this;
    }
}

/**
 * The deferred outputs behind one `$outLane` call. Every channel, control,
 * and range bound is deferred because the out may be declared after the tap;
 * the audio is fixed at two channels so consumers can derive channel counts
 * at call time.
 */
export class OutLaneTap {
    readonly lane: OutLane;
    private readonly audio: DeferredCollection;
    private readonly active: DeferredModuleOutput;
    private readonly level: DeferredModuleOutput;
    private readonly levelMin: DeferredModuleOutput;
    private readonly levelMax: DeferredModuleOutput;
    private readonly pan: DeferredModuleOutput;
    private readonly panMin: DeferredModuleOutput;
    private readonly panMax: DeferredModuleOutput;

    constructor(builder: GraphBuilder) {
        const deferred = () => new DeferredModuleOutput(builder);
        this.audio = new DeferredCollection(deferred(), deferred());
        this.active = deferred();
        this.level = deferred();
        this.levelMin = deferred();
        this.levelMax = deferred();
        this.pan = deferred();
        this.panMin = deferred();
        this.panMax = deferred();
        this.lane = new OutLane([...this.audio], {
            active: this.active.withRange(0, 5),
            level: this.level.withRange(this.levelMin, this.levelMax),
            pan: this.pan.withRange(this.panMin, this.panMax),
        });
    }

    /** Point the lane at its out's signals; mono audio fills both channels. */
    resolve(sources: OutLaneSources): void {
        this.audio.set(sources.audio);
        this.active.set(sources.active);
        this.level.set(sources.level.output);
        this.levelMin.set(sources.level.min);
        this.levelMax.set(sources.level.max);
        this.pan.set(sources.pan.output);
        this.panMin.set(sources.pan.min);
        this.panMax.set(sources.pan.max);
    }
}

/** `$outLane(label)`: tap the out with that label. */
export function createOutLane(builder: GraphBuilder, label: string): OutLane {
    if (typeof label !== 'string' || label.length === 0) {
        throw new Error('$outLane: label must be a non-empty string');
    }
    const tap = new OutLaneTap(builder);
    builder.addOutLaneTap(label, tap, captureSourceLocation());
    return tap.lane;
}
