import vm from 'node:vm';
import type { ModuleSchema, PatchGraph } from '@modular/core';
import {
    DSLContext,
    hz,
    note,
    setActiveSpanRegistry,
    setDSLWrapperLineOffset,
    captureSourceLocation,
    deriveChannelCountChecked,
} from './factories';
import type {
    BufferOutputRef,
    GrooveTemplate,
    Signal,
    PolySignal,
    SourceLocation,
    Collection,
    CollectionWithRange,
} from './GraphBuilder';
import {
    $r,
    $cartesian,
    BaseCollection,
    DeferredModuleOutput,
    DeferredCollection,
    Bus,
    ModuleOutput,
    cycleToChannels,
    replaceSignals,
    PORT_MAX_CHANNELS,
} from './GraphBuilder';
import { $g1, $g2, $g3 } from './signalGroups';
import { createOutLane, type OutLane } from './outLane';
import { analyzeSourceSpans } from './analyzeSource';
import type { CallSiteSpanRegistry } from './analyzeSource';
import type { InterpolationResolutionMap } from '../../shared/dsl/spanTypes';
import {
    FIRST_LINE_COLUMN_OFFSET,
    setActiveInterpolationResolutions,
} from '../../shared/dsl/spanTypes';
import type { SliderDefinition } from '../../shared/dsl/sliderTypes';
import type { ButtonDefinition } from '../../shared/dsl/buttonTypes';
import type { EqPanelDef } from '../../shared/dsl/eqPanelTypes';
import { GATE_HIGH_VOLTAGE } from '../../shared/dsl/buttonTypes';
import { assertControlsPlaced, createControls } from './controls';
import { $p, setActiveCycleFactory, type CycleFactory } from './miniNotation';
import { isPatternValue } from './patternKinds';
import { VideoGraphBuilder } from './video/VideoGraphBuilder';
import type { VideoOutput } from './video/VideoOutput';
import type {
    CompiledVideoShader,
    VideoPreviewSite,
} from '../../shared/video/videoGraph';
import { compileVideoGraph } from './video/wgslCompiler';
import { VIDEO_DOCS } from '../../shared/dsl/videoDocs';

// Augment Array.prototype with pipe() for TypeScript
declare global {
    interface Array<T> {
        pipe<U>(this: this, pipelineFunc: (self: this) => U): U;
    }
}

/**
 * Result of executing a DSL script.
 */
export interface DSLExecutionResult {
    /** The generated patch graph */
    patch: PatchGraph;
    /** Map from module ID to source location in DSL code */
    sourceLocationMap: Map<string, SourceLocation>;
    /** Interpolation resolution map for template literal const redirects */
    interpolationResolutions: InterpolationResolutionMap;
    /** Slider definitions created by $slider() DSL function calls */
    sliders: SliderDefinition[];
    /** Button definitions created by $btn()/$toggleBtn() DSL function calls */
    buttons: ButtonDefinition[];
    /** Full call expression spans for DSL methods (.scope(), $slider(), etc.) */
    callSiteSpans: CallSiteSpanRegistry;
    /** Inline panels requested by `ui: true` EQs */
    eqPanels: EqPanelDef[];
    /** Shader for the video renderer; null when the patch has no `$v.out` or `$v.preview` */
    video: CompiledVideoShader | null;
    /** One entry per `$v.preview` call, in the order the shader draws them */
    videoPreviews: VideoPreviewSite[];
}

export interface WavsFolderNode {
    [name: string]: WavsFolderNode | 'file';
}

/** What the engine reports about a decoded WAV or media audio track. */
interface LoadedWav {
    channels: number;
    frameCount: number;
    path: string;
    sampleRate: number;
    duration: number;
    bitDepth: number;
    pitch?: number | null;
    playback?: string | null;
    bpm?: number | null;
    beats?: number | null;
    timeSignature?: { num: number; den: number } | null;
    barCount?: number | null;
    loops: Array<{ loopType: string; start: number; end: number }>;
    cuePoints: Array<{ position: number; label: string }>;
    mtime: number;
}

/** The `wav` parameter value that refers to decoded audio held by the engine. */
function wavRef(path: string, info: LoadedWav) {
    return {
        type: 'wav_ref' as const,
        path,
        channels: info.channels,
        sampleRate: info.sampleRate,
        frameCount: info.frameCount,
        duration: info.duration,
        bitDepth: info.bitDepth,
        mtime: info.mtime,
        ...(info.pitch != null && { pitch: info.pitch }),
        ...(info.playback != null && { playback: info.playback }),
        ...(info.bpm != null && { bpm: info.bpm }),
        ...(info.beats != null && { beats: info.beats }),
        ...(info.timeSignature != null && {
            timeSignature: {
                num: info.timeSignature.num,
                den: info.timeSignature.den,
            },
        }),
        ...(info.barCount != null && { barCount: info.barCount }),
        loops: info.loops.map((l) => ({
            type: l.loopType as 'forward' | 'pingpong' | 'backward',
            start: l.start,
            end: l.end,
        })),
        cuePoints: info.cuePoints.map((c) => ({
            position: c.position,
            label: c.label,
        })),
    };
}
export interface DSLExecutionOptions {
    sampleRate?: number;
    /**
     * Channel count of the audio input device, which is how wide `$input` is
     * built. Read at execution time, so a device swap reaches `$input` on the
     * next run of the patch. Clamped to 1..PORT_MAX_CHANNELS, and defaults to
     * the maximum for callers with no device to report — `$input` never
     * truncates silently, it only narrows when a host says how wide the
     * device is.
     */
    inputChannels?: number;
    /** Whether a workspace-relative path names an existing file, for media errors. */
    mediaExists?: (path: string) => boolean;
    workspaceRoot?: string | null;
    wavsFolderTree?: WavsFolderNode | null;
    loadWav?: (path: string) => LoadedWav;
    /**
     * Decodes the audio track of a workspace media file, or returns null when
     * it has none. The returned `path` names the decoded audio for a `Wav`.
     */
    loadMediaAudio?: (path: string) => LoadedWav | null;
}

// Install pipe() on Array.prototype so arrays in the DSL can use it.
// Non-enumerable to avoid polluting for-in loops.
if (typeof Array.prototype.pipe !== 'function') {
    Object.defineProperty(Array.prototype, 'pipe', {
        configurable: true,
        enumerable: false,
        value: function pipe<T>(
            this: unknown,
            pipelineFunc: (self: typeof this) => T,
        ): T {
            return pipelineFunc(this);
        },
        writable: true,
    });
}

/**
 * Execute a DSL script and return the resulting PatchGraph with source locations.
 */
export function executePatchScript(
    source: string,
    schemas: ModuleSchema[],
    options: DSLExecutionOptions = {},
): DSLExecutionResult {
    // Create DSL context
    // Console.log('Executing DSL script with schemas:', schemas);
    const context = new DSLContext(schemas);
    const sampleRate = options.sampleRate ?? 48_000;

    // Create the execution environment with all DSL functions
    // Remove _clock from user-facing namespace (it's internal, used only for ROOT_CLOCK)
    const {
        _clock,
        _videoTap,
        _mediaAudio,
        _smoothControl,
        ...userNamespaceTree
    } = context.namespaceTree;

    if (typeof _clock !== 'function') {
        throw new Error(
            'DSL execution error: "_clock" module not found in schemas',
        );
    }

    const signal = context.namespaceTree['$signal'];
    if (typeof signal !== 'function') {
        throw new Error(
            'DSL execution error: "$signal" module not found in schemas',
        );
    }

    // Create default clock module that runs at 120 BPM
    const $clock = _clock(120, 4, 4, {
        id: 'ROOT_CLOCK',
    });

    const inputChannels = Math.min(
        Math.max(options.inputChannels ?? PORT_MAX_CHANNELS, 1),
        PORT_MAX_CHANNELS,
    );
    const rootInput = signal(
        Array.from({ length: inputChannels }, (_, i) => ({
            channel: i,
            module: 'HIDDEN_AUDIO_IN',
            port: 'input',
            type: 'cable',
        })),
        { id: 'ROOT_INPUT' },
    );

    // Create functions to set global tempo and output gain
    const builder = context.getBuilder();
    const $setTempo = (tempo: number) => {
        if (
            typeof tempo !== 'number' ||
            !Number.isFinite(tempo) ||
            tempo <= 0
        ) {
            throw new Error(
                `$setTempo: tempo must be a finite number greater than 0, got ${tempo}`,
            );
        }
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

    // Shape is checked here; step ordering and ranges are validated in Rust.
    const $setGroove = (template: GrooveTemplate) => {
        if (
            typeof template !== 'object' ||
            template === null ||
            !Array.isArray(template.timing)
        ) {
            throw new Error(
                '$setGroove: expected a groove template with a `timing` array, e.g. $setGroove($swing(0.6))',
            );
        }
        builder.setGroove(template);
    };

    const $swing = (
        ratio: number,
        config: { grid?: number; humanize?: number } = {},
    ): GrooveTemplate => {
        if (typeof ratio !== 'number' || !(ratio > 0 && ratio < 1)) {
            throw new Error(
                `$swing: ratio must be a number between 0 and 1 (0.5 is straight), got ${ratio}`,
            );
        }
        if (
            typeof config !== 'object' ||
            config === null ||
            Array.isArray(config)
        ) {
            throw new Error(
                '$swing: expected a config object as the second argument, e.g. $swing(0.66, { grid: 8 })',
            );
        }
        const { grid = 16, humanize = 0 } = config;
        if (typeof humanize !== 'number' || !(humanize >= 0)) {
            throw new Error(
                `$swing: humanize must be a number 0 or greater, got ${humanize}`,
            );
        }
        // The pair spans two grid steps; the second starts `2 * ratio` steps in.
        const template: GrooveTemplate = { grid, timing: [0, 2 * ratio - 1] };
        if (humanize > 0) {
            template.random = humanize;
        }
        return template;
    };

    /**
     * Create a DeferredCollection with placeholder signals that can be assigned later.
     * Useful for feedback loops and forward references.
     * @param channels - Number of deferred outputs (1-64, default 1)
     */
    const $deferred = (channels: number = 1): DeferredCollection => {
        if (channels < 1 || channels > PORT_MAX_CHANNELS) {
            throw new Error(
                `deferred() channels must be between 1 and ${PORT_MAX_CHANNELS}, got ${channels}`,
            );
        }
        const items: DeferredModuleOutput[] = [];
        for (let i = 0; i < channels; i++) {
            items.push(new DeferredModuleOutput(builder));
        }
        return new DeferredCollection(...items);
    };

    const $outLane = (label: string): OutLane => createOutLane(builder, label);

    const $bus = (cb: (mixed: Collection) => unknown): Bus =>
        new Bus(builder, cb);

    const $setEndOfChainCb = (
        cb: (
            mixed: Collection,
        ) => ModuleOutput | Collection | CollectionWithRange,
    ) => {
        builder.setEndOfChainCb(cb);
    };

    const $buffer = (
        input: ModuleOutput | Collection | number,
        lengthSeconds: number,
        config?: { id?: string },
    ): BufferOutputRef => {
        if (
            typeof lengthSeconds !== 'number' ||
            !Number.isFinite(lengthSeconds)
        ) {
            throw new Error('$buffer() lengthSeconds must be a finite number');
        }
        if (lengthSeconds <= 0) {
            throw new Error(
                `$buffer() lengthSeconds must be greater than 0, got ${lengthSeconds}`,
            );
        }

        const sourceLocation = captureSourceLocation();

        // Create a $buffer module in the graph
        const node = builder.addModule('$buffer', config?.id, sourceLocation);

        // Resolve the input signal and set params
        const resolvedInput = replaceSignals(input);
        node._setParam('input', resolvedInput);
        node._setParam('length', lengthSeconds);

        // Derive channel count from the input signal
        const derivedChannels = deriveChannelCountChecked(
            '$buffer',
            node.getParamsSnapshot(),
            sourceLocation,
        );
        if (derivedChannels !== undefined) {
            node._setDerivedChannelCount(derivedChannels);
        }
        const channels = derivedChannels ?? 1;

        const frameCount = Math.max(1, Math.ceil(lengthSeconds * sampleRate));

        return {
            type: 'buffer_ref',
            module: node.id,
            port: 'buffer',
            channels,
            frameCount,
        };
    };

    const $cycle = userNamespaceTree['$cycle'];
    if (typeof $cycle !== 'function') {
        throw new Error(
            'DSL execution error: "$cycle" module not found in schemas',
        );
    }
    const cycleFactory: CycleFactory = $cycle;
    const $mix = userNamespaceTree['$mix'];
    const $delayRead = userNamespaceTree['$delayRead'];
    const $clamp = userNamespaceTree['$clamp'];
    if (
        typeof $mix !== 'function' ||
        typeof $delayRead !== 'function' ||
        typeof $clamp !== 'function'
    ) {
        throw new Error(
            'DSL execution error: "$delay" requires "$mix", "$delayRead", and "$clamp" modules',
        );
    }

    interface DelayOptions {
        /**
         * Feedback amount, 0–5 (0 = no repeats, 5 = 100% = unity, default
         * 2.5 = 50%). Clamped to 0–5. May be a signal for modulation.
         */
        feedback?: PolySignal;
        /**
         * Optional processor applied to the dry-plus-feedback mix before it is
         * written to the buffer — e.g. a filter or saturator that colours every
         * recirculation of the delay.
         */
        feedbackCb?: (mixed: Collection) => Collection | ModuleOutput;
        /**
         * Maximum delay time in seconds — sizes the underlying buffer and
         * caps how long `time` can be (default 5).
         */
        maxTime?: number;
    }

    // Channel count of a polysignal argument: collections and arrays
    // contribute their (flattened) length; a single output or scalar counts as
    // one. Used to size the feedback `$deferred` to the widest param.
    const polyChannels = (v: unknown): number => {
        if (v instanceof BaseCollection) return Math.max(1, v.length);
        if (Array.isArray(v)) {
            return v.reduce((n, e) => n + polyChannels(e), 0);
        }
        return 1;
    };

    /**
     * Delay with feedback: sum `input` with the attenuated feedback, optionally
     * process that mix through `feedbackCb`, write it into a `$buffer` of
     * `maxTime` seconds, and read it back `time` seconds late. The delayed read
     * is attenuated by `feedback` and summed back in, so a `feedbackCb` filter
     * colours every recirculation. With `feedback = 0` you still hear the first
     * echo; higher values repeat it.
     *
     * Returns the wet `$delayRead` output with the captured `buffer` attached
     * as a property for additional taps. Mix the dry signal back yourself
     * (`$mix([input, $delay(input, time)])`) or use `.$m.delay(mix, time)`.
     */
    const $delay = (
        input: Collection | ModuleOutput,
        time: PolySignal,
        opts?: DelayOptions,
    ): Collection & { buffer: BufferOutputRef } => {
        const feedback = $clamp(opts?.feedback ?? 2.5, {
            min: 0,
            max: 5,
        }) as Collection;
        const maxTime = opts?.maxTime ?? 5;
        // Size the feedback path to the widest polysignal param so a poly
        // `time` or `feedback` is not truncated when it exceeds `input`.
        const channels = Math.max(
            1,
            polyChannels(input),
            polyChannels(time),
            polyChannels(opts?.feedback ?? 2.5),
        );
        const def = $deferred(channels);
        // Broadcast a narrower input across the feedback width so every channel
        // is excited by the dry signal (otherwise the extra channels carry
        // only feedback and decay to silence — `$mix` skips, not cycles, a
        // narrower input group's missing channels).
        const mixed = $mix([
            cycleToChannels(input, channels),
            def,
        ]) as Collection;
        // Colour every recirculation: process the dry-plus-feedback mix before
        // it is written to the buffer.
        const buffered = opts?.feedbackCb ? opts.feedbackCb(mixed) : mixed;
        const buf = $buffer(buffered, maxTime);
        const delayed = $delayRead(buf, time) as Collection;
        def.set(delayed.amp(feedback as unknown as Signal));
        return Object.assign(delayed, { buffer: buf });
    };

    // Expose `$delay` on the `.$` / `.$m` chainable namespaces. It is DSL
    // sugar rather than a module schema, so it is registered explicitly here
    // instead of being discovered via `qualifiesForDollarChain`.
    builder.registerDollarMethod('delay', (self, ...args) =>
        $delay(
            self as Collection | ModuleOutput,
            ...(args as [PolySignal, DelayOptions?]),
        ),
    );

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

    const $xover = userNamespaceTree['$xover'];
    const $comp = userNamespaceTree['$comp'];
    const $scaleAndShift = userNamespaceTree['$scaleAndShift'];
    if (
        typeof $xover !== 'function' ||
        typeof $comp !== 'function' ||
        typeof $scaleAndShift !== 'function'
    ) {
        throw new Error(
            'DSL execution error: "$ott" requires "$xover", "$comp", and "$scaleAndShift" modules',
        );
    }

    /**
     * `$ott` — three-band upward + downward compressor in the style of Xfer's
     * OTT. Splits the input into low/mid/high via `$xover`, applies aggressive
     * upward + downward compression to each band with `$comp`, sums the bands,
     * and crossfades against the original input via `depth`.
     *
     * Per-band trim (`lowGain` / `midGain` / `highGain`) uses `$scaleAndShift`
     * convention: 5 V = unity, 0 V = silence, 10 V = +6 dB.
     *
     * @example
     * $ott(drums).out()
     *
     * @example
     * // tame highs, push lows
     * $ott(bus, { depth: 4, lowGain: 6, highGain: 4, threshold: 1.5 }).out()
     */
    const $ott = (input: PolySignal, config: OttConfig = {}): Collection => {
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

        const bands = $xover(input, xoverConf) as Collection & {
            low: Collection;
            mid: Collection;
            high: Collection;
        };

        // Split the side-chain through an identical crossover so each band's
        // compressor keys off the matching frequency range of the sidechain.
        const scBands =
            config.sidechain !== undefined
                ? ($xover(config.sidechain, xoverConf) as Collection & {
                      low: Collection;
                      mid: Collection;
                      high: Collection;
                  })
                : null;

        const lowComp = $comp(bands.low, {
            ...compConf,
            ...(scBands !== null && { sidechain: scBands.low }),
        });
        const midComp = $comp(bands.mid, {
            ...compConf,
            ...(scBands !== null && { sidechain: scBands.mid }),
        });
        const highComp = $comp(bands.high, {
            ...compConf,
            ...(scBands !== null && { sidechain: scBands.high }),
        });

        // Per-band trim — $scaleAndShift convention: scale=5 → unity gain.
        const low = $scaleAndShift(lowComp, config.lowGain ?? 5, 0);
        const mid = $scaleAndShift(midComp, config.midGain ?? 5, 0);
        const high = $scaleAndShift(highComp, config.highGain ?? 5, 0);

        const wet = $mix([low, mid, high]) as Collection;

        // Crossfade dry vs wet using `depth` (0–5 V):
        //   dryWeight = 5 − depth → unity when depth=0, silence when depth=5
        //   wetWeight = depth      → silence when depth=0, unity when depth=5
        const depth = config.depth ?? 5;
        const dryWeight = $scaleAndShift(depth, -5, 5);

        return $mix([
            $scaleAndShift(input, dryWeight as Signal, 0),
            $scaleAndShift(wet, depth, 0),
        ]) as Collection;
    };

    const $track = userNamespaceTree['$track'];
    if (typeof $track !== 'function') {
        throw new Error(
            'DSL execution error: "$cross" requires "$track" module',
        );
    }

    /**
     * @example
     * // crossfade 3 voices by a slow LFO
     * const osc = $sine(['c', 'e', 'g'])
     * const weights = $cross(osc.length, $sine('0.25hz').range(0, 1));
     * osc.amp(weights).out();
     */
    const $cross = (
        count: number,
        playhead: Signal,
        range: [number, number] = [0, 5],
        interpolationType?: string,
    ): Collection => {
        const frames: [Signal[], number][] = [];
        for (let i = 0; i < count; i++) {
            const frame: Signal[] = Array.from(
                { length: count },
                () => range[0],
            );
            frame[i] = range[1];
            frames.push([frame, count > 1 ? i / (count - 1) : 0]);
        }
        return $track(frames, { playhead, interpolationType }) as Collection;
    };

    if (typeof _smoothControl !== 'function') {
        throw new Error(
            'DSL execution error: "_smoothControl" module not found in schemas',
        );
    }

    // Control factories; their collectors fill as the patch calls them.
    const controlFactories = { $signal: signal, _smoothControl };
    const controls = createControls({
        rangedSignal: (moduleType, value, id, min, max) =>
            builder
                .$c(controlFactories[moduleType](value, { id }))
                .withRange(min, max),
    });

    if (typeof _videoTap !== 'function') {
        throw new Error(
            'DSL execution error: "_videoTap" module not found in schemas',
        );
    }
    if (typeof _mediaAudio !== 'function') {
        throw new Error(
            'DSL execution error: "_mediaAudio" module not found in schemas',
        );
    }
    const videoBuilder = new VideoGraphBuilder({
        controlValue: (moduleId) => {
            const slider = controls.sliders.find(
                (s) => s.moduleId === moduleId,
            );
            if (slider) return slider.value;
            const button = controls.buttons.find(
                (b) => b.moduleId === moduleId,
            );
            if (button) return button.value ? GATE_HIGH_VOLTAGE : 0;
            return undefined;
        },
        playPattern: (pattern) =>
            cycleFactory(pattern) as
                | ModuleOutput
                | BaseCollection<ModuleOutput>,
        publishTap: (output, slot) => {
            _videoTap(output, slot);
        },
        cvSignal: (id) => builder.$c(signal(0, { id })).withRange(0, 1),
        mediaAudio: (path, playback) => {
            if (!options.loadMediaAudio) {
                throw new Error(
                    '$v.video(...).audio: media audio loading is not available',
                );
            }
            const info = timedWavLoad(() => options.loadMediaAudio!(path));
            if (info === null) {
                throw new Error(`$v.video(...).audio: "${path}" has no audio`);
            }
            return _mediaAudio(wavRef(info.path, info), playback) as Collection;
        },
        mediaExists: options.mediaExists,
        sourceLocation: captureSourceLocation,
        constantSignal: (value) => signal(value) as Collection,
    });

    /** `$v.scopeXY(...)` shown as the patch's picture. */
    const $scopeXY = (
        ...args: Parameters<VideoGraphBuilder['scopeXY']>
    ): VideoOutput => videoBuilder.out(videoBuilder.scopeXY(...args));

    /**
     * Load WAV samples from the wavs/ folder.
     * Returns a proxy tree matching the folder structure; leaf nodes trigger
     * loadWav() and return `{ type: 'wav_ref', path, channels }` objects.
     */
    // Wall-clock time this run spends inside synchronous loadWav host calls.
    // The vm timeout below measures total wall-clock time of the run, so this
    // distinguishes a patch stuck in a loop from one legitimately spending
    // its budget decoding samples from disk. `wavLoadStart` marks a load in
    // flight: the timeout terminates all JS in the isolate, so the
    // post-load accumulation may never run and the catch block below must
    // count the interrupted load itself.
    let wavLoadMs = 0;
    let wavLoadStart: number | null = null;

    /** Runs a synchronous host load, billing its time to WAV loading. */
    const timedWavLoad = <T>(load: () => T): T => {
        const loadStart = performance.now();
        wavLoadStart = loadStart;
        try {
            return load();
        } finally {
            // Also on throw: the time was still spent loading, and a
            // stale wavLoadStart would bill everything after a caught
            // load failure — even an infinite loop — to WAV loading.
            wavLoadMs += performance.now() - loadStart;
            wavLoadStart = null;
        }
    };

    const $wavs = (): unknown => {
        const tree = options.wavsFolderTree;
        if (!tree) {
            return new Proxy(
                {},
                {
                    get(_target, prop) {
                        throw new Error(
                            `$wavs().${String(prop)}: no wavs/ folder found in workspace`,
                        );
                    },
                },
            );
        }

        // Memoize the lexicographically-sorted list of direct file basenames
        // per folder node, so numeric-index resolution sorts at most once
        // per node regardless of how many `$wavs()[i]` calls happen.
        const sortedFilesCache = new WeakMap<WavsFolderNode, string[]>();
        function sortedFileList(node: WavsFolderNode): string[] {
            const cached = sortedFilesCache.get(node);
            if (cached) return cached;
            const files = Object.entries(node)
                .filter(([, v]) => v === 'file')
                .map(([k]) => k)
                .sort((a, b) => a.localeCompare(b));
            sortedFilesCache.set(node, files);
            return files;
        }

        function makeProxy(node: WavsFolderNode, pathParts: string[]): unknown {
            // Resolve a known file leaf (basename `fileName` exists in `node`
            // as a `'file'`) into a `WavHandle`. Single source of truth
            // shared by named-key access and numeric-index access.
            function loadFile(fileName: string): unknown {
                const relPath = [...pathParts, fileName].join('/');
                if (!options.loadWav) {
                    throw new Error('$wavs(): loadWav function not provided');
                }
                return wavRef(
                    relPath,
                    timedWavLoad(() => options.loadWav!(relPath)),
                );
            }

            return new Proxy(
                {},
                {
                    get(_target, prop) {
                        if (typeof prop !== 'string') return undefined;

                        // Numeric index access wraps modulo the file count of
                        // this folder. Only direct files participate
                        // (subfolders excluded — they get their own index).
                        if (/^-?(0|[1-9][0-9]*)$/.test(prop)) {
                            const files = sortedFileList(node);
                            if (files.length === 0) {
                                const fullPath = [
                                    ...pathParts,
                                    `[${prop}]`,
                                ].join('/');
                                throw new Error(
                                    `$wavs(): "${fullPath}" — no wav files in this folder to index into`,
                                );
                            }
                            const i = parseInt(prop, 10);
                            const wrapped =
                                ((i % files.length) + files.length) %
                                files.length;
                            return loadFile(files[wrapped]);
                        }

                        const child = node[prop];
                        if (child === undefined) {
                            const fullPath = [...pathParts, prop].join('/');
                            throw new Error(
                                `$wavs(): "${fullPath}" not found. Available: ${Object.keys(node).join(', ') || '(empty)'}`,
                            );
                        }

                        if (child === 'file') {
                            return loadFile(prop);
                        }

                        // Directory node — return nested proxy
                        return makeProxy(child, [...pathParts, prop]);
                    },
                    ownKeys() {
                        return Object.keys(node);
                    },
                    getOwnPropertyDescriptor(_target, prop) {
                        if (typeof prop === 'string' && prop in node) {
                            return {
                                configurable: true,
                                enumerable: true,
                                writable: false,
                            };
                        }
                        return undefined;
                    },
                },
            );
        }

        return makeProxy(tree, []);
    };

    /**
     * $phase.* DSL helpers produce phase-warp descriptors for modules that
     * accept a `PhaseWarp` (`$wavetable`, `$ensemble`, `$pWarp`).
     *
     * Each helper returns a plain JSON object whose shape matches the Rust
     * `PhaseWarp` deserializer (`#[serde(tag = "type", rename_all = "camelCase")]`).
     *
     * Inner signal-valued fields are passed through `replaceSignals` so that
     * ModuleOutputs / Collections are converted to the same wire format used
     * for module-factory params. This matches the existing mechanism used by
     * `_setParam` in GraphBuilder.
     *
     * Warps are composable: each returned descriptor has a `.pipe(fn)` method,
     * and the optional second argument to each helper feeds this warp's output
     * phase into `next`.
     */
    function wrapWarp(descriptor: Record<string, unknown>): Record<
        string,
        unknown
    > & {
        pipe: <T>(fn: (self: Record<string, unknown>) => T) => T;
    } {
        const t = { ...descriptor } as Record<string, unknown> & {
            pipe: <T>(fn: (self: Record<string, unknown>) => T) => T;
        };
        Object.defineProperty(t, 'pipe', {
            value: <T>(fn: (self: typeof t) => T): T => fn(t),
            enumerable: false,
            writable: false,
            configurable: false,
        });
        return t;
    }

    // A pattern argument plays through its own `$cycle` before it becomes a
    // warp signal.
    const warpSignal = (value: unknown) =>
        replaceSignals(isPatternValue(value) ? cycleFactory(value) : value);

    const warpHelper =
        (type: string, field: string) => (value: unknown, next?: unknown) => {
            const t = wrapWarp({ type, [field]: warpSignal(value) });
            return next !== undefined
                ? wrapWarp({ type: 'pipe', first: t, second: next })
                : t;
        };

    const $phase = {
        mirror: warpHelper('mirror', 'amount'),
        bend: warpHelper('bend', 'amount'),
        sync: warpHelper('sync', 'ratio'),
        fold: warpHelper('fold', 'amount'),
        pwm: warpHelper('pwm', 'width'),
        pulsar: warpHelper('pulsar', 'amount'),
        crush: warpHelper('crush', 'amount'),
        feedback: warpHelper('feedback', 'amount'),
    };

    const dslGlobals = {
        // Prefixed namespace tree (modules and namespaces, minus _clock)
        ...userNamespaceTree,
        // Helper functions with $ prefix
        $hz: hz,
        $note: note,
        // Mini-notation parser — wraps a string in a ParsedPattern that
        // $cycle consumes as a positional argument. Carries
        // $p.s(source, scale) for scale-degree patterns (integer degrees
        // resolved through a scale string into voltage atoms for $cycle).
        $p,
        // Phase-warp descriptors for $wavetable, $ensemble and $pWarp
        $phase,
        // Collection helpers
        $c: builder.$c.bind(builder),
        $r,
        $cartesian,
        // Cartesian signal groups — tag a param value so groups multiply
        // across a module instead of cycling together
        $g1,
        $g2,
        $g3,
        // Deferred signal helper
        $deferred,
        // Panel controls
        $slider: controls.$slider,
        $btn: controls.$btn,
        $toggleBtn: controls.$toggleBtn,
        $cGroup: controls.$cGroup,
        // Bus
        $bus,
        $outLane,
        // Global settings
        $setTempo,
        $setOutputGain,
        $setTimeSignature,
        $setGroove,
        $swing,
        $setEndOfChainCb,
        // XY background oscilloscope
        $scopeXY,
        // Video synthesis
        $v: Object.fromEntries(
            VIDEO_DOCS.map(({ name }) => [
                name,
                videoBuilder[name as keyof VideoGraphBuilder],
            ]),
        ),
        $buffer,
        $delay,
        $ott,
        $cross,
        // WAV sample loading
        $wavs,
        // Built-in modules
        $clock,
        $input: rootInput,
    };

    // Console.log(dslGlobals);

    // Build the script body - count wrapper lines for source mapping.
    // Stack-trace line numbers are relative to the compiled vm script: the
    // template literal's leading newline, the 'use strict' line, and the
    // IIFE opener put user code at line 4 in stack traces.
    const wrapperLineCount = 3;
    setDSLWrapperLineOffset(wrapperLineCount);

    // The function body template indents the first line of source with 4 spaces
    // This affects the column reported by V8 for the first line only
    const firstLineColumnOffset = FIRST_LINE_COLUMN_OFFSET;

    // Analyze source code to extract argument spans before execution
    // The registry maps call-site keys (line:column) to argument span info
    const {
        registry: spanRegistry,
        interpolationResolutions,
        callSiteSpans,
    } = analyzeSourceSpans(
        source,
        schemas,
        wrapperLineCount,
        firstLineColumnOffset,
    );
    setActiveSpanRegistry(spanRegistry);
    setActiveInterpolationResolutions(interpolationResolutions);
    setActiveCycleFactory(cycleFactory);

    // The user source runs as a function body (an IIFE), not at the script's
    // top level, so top-level `return` is legal in a patch script.
    const scriptBody = `
    'use strict';
    (function () {
    ${source}
    })();
  `;

    // Cap on synchronous script execution. A non-terminating patch script
    // (e.g. an accidental infinite loop) raises a catchable error instead of
    // blocking the Electron main process forever.
    const executionTimeoutMs = 5000;

    try {
        // Execute the script in a vm context so the timeout applies. The DSL
        // globals are the sandbox's globals; they are host objects, so
        // instanceof checks against host classes keep working. The default
        // script filename contains `<anonymous>`, which captureSourceLocation
        // relies on to find DSL stack frames.
        const script = new vm.Script(scriptBody);
        const sandbox = vm.createContext({
            ...dslGlobals,
            console,
            structuredClone,
        });
        // The sandbox is its own realm: array literals in the patch script use
        // the sandbox's Array.prototype, so pipe() must be installed there too
        // (mirroring the host-side installation above). The explicit filename
        // keeps `<anonymous>` out of the installer's stack frames, so
        // captureSourceLocation never mistakes the installer's `pipe` frame
        // for the user's call site.
        vm.runInContext(
            `Object.defineProperty(Array.prototype, 'pipe', {
                configurable: true,
                enumerable: false,
                value: function pipe(pipelineFunc) { return pipelineFunc(this); },
                writable: true,
            });`,
            sandbox,
            { filename: 'dsl-pipe-installer.js' },
        );
        script.runInContext(sandbox, { timeout: executionTimeoutMs });
        assertControlsPlaced(source, [
            ...controls.sliders,
            ...controls.buttons,
            ...controls.groups,
        ]);

        // The video graph goes first: building it publishes the audio taps
        // it reads into the patch.
        const videoGraph = videoBuilder.build();
        const resultBuilder = context.getBuilder();
        const patch = resultBuilder.toPatch();
        const sourceLocationMap = resultBuilder.getSourceLocationMap();

        return {
            buttons: controls.buttons,
            callSiteSpans,
            eqPanels: resultBuilder.getEqPanels(),
            interpolationResolutions,
            patch,
            sliders: controls.sliders,
            sourceLocationMap,
            video: videoGraph === null ? null : compileVideoGraph(videoGraph),
            videoPreviews: videoBuilder.getPreviewSites(),
        };
    } catch (error) {
        if (
            (error as NodeJS.ErrnoException | null)?.code ===
            'ERR_SCRIPT_EXECUTION_TIMEOUT'
        ) {
            // The timeout counts wall-clock time, including synchronous host
            // call-outs. When most of the budget went to loading WAV files
            // (slow/cold disk, big library), say so — the wav cache keeps
            // everything already decoded, so a re-run makes progress.
            const seconds = executionTimeoutMs / 1000;
            const totalWavMs =
                wavLoadMs +
                (wavLoadStart !== null ? performance.now() - wavLoadStart : 0);
            const message =
                totalWavMs >= executionTimeoutMs / 2
                    ? `DSL execution error: Patch script timed out after ${seconds}s, ${(totalWavMs / 1000).toFixed(1)}s of it loading WAV files — loaded files stay cached, so run again to continue`
                    : `DSL execution error: Patch script timed out after ${seconds}s — check for infinite loops`;
            throw new Error(message, { cause: error });
        }
        // Errors thrown by code inside the vm are sandbox-realm Error
        // instances, so duck-type on `message` rather than instanceof.
        const message =
            typeof error === 'object' &&
            error !== null &&
            typeof (error as { message?: unknown }).message === 'string'
                ? (error as { message: string }).message
                : null;
        if (message !== null) {
            throw new Error(`DSL execution error: ${message}`, {
                cause: error,
            });
        }
        throw error;
    } finally {
        // Clear the span registry after execution — spans are already baked into
        // Module state via ARGUMENT_SPANS_KEY so the registry isn't needed anymore.
        setActiveSpanRegistry(null);
        setActiveCycleFactory(null);
        // NOTE: Do NOT clear interpolation resolutions here. They are read
        // Asynchronously by moduleStateTracking during decoration polling and
        // Must persist until the next execution replaces them.
    }
}
