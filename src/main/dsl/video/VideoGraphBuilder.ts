import {
    MAX_VIDEO_TAPS,
    sameSource,
    type VideoGraph,
    type VideoHistory,
    VIDEO_HISTORY_LEN,
    type VideoNode,
    type VideoPreview,
    type VideoPreviewSite,
    type VideoSourceDef,
    type VideoUniform,
    type VideoValue,
} from '../../../shared/video/videoGraph';
import {
    BaseCollection,
    type Collection,
    type CollectionWithRange,
    ModuleOutput,
} from '../GraphBuilder';
import { isPatternValue } from '../patternKinds';
import { VideoOutput, type VideoOps } from './VideoOutput';
import { colorMethods } from './videoColor';
import { filterMethods } from './videoFilters';
import { generatorMethods } from './videoGenerators';
import { mathMethods } from './videoMath';
import { memoryMethods } from './videoMemory';
import { modulatorMethods } from './videoModulators';
import { sourceMethods } from './videoSources';
import { warpMethods } from './videoWarps';
import {
    describe,
    type AudioSignal,
    type VideoInputs,
    type VideoNodeExtra,
    type VideoAudioConfig,
    type VideoAudioPlayback,
    type VideoCore,
    type VideoCvConfig,
    type VideoGraphHost,
    type VideoPreviewConfig,
    type VideoSource,
} from './videoBuilderTypes';

/** `Omit` applied to each member of a union. */
type DistributiveOmit<T, K extends PropertyKey> = T extends unknown
    ? Omit<T, K>
    : never;

const PREVIEW_VIEWS: readonly string[] = ['image', 'waveform', 'vectorscope'];

/** WebGPU's default `maxSampledTexturesPerShaderStage`, which the renderer does not raise. */
const MAX_SAMPLED_TEXTURES = 16;

/**
 * Collects `$v.*` calls into a {@link VideoGraph}. The functions that only
 * build nodes live in the `video*.ts` files beside it and are mixed in; this
 * class holds the graph state and the functions that touch it.
 */
// The mixed-in groups are typed by merging their return types into the class.
// eslint-disable-next-line @typescript-eslint/no-unsafe-declaration-merging
export interface VideoGraphBuilder
    extends
        ReturnType<typeof generatorMethods>,
        ReturnType<typeof mathMethods>,
        ReturnType<typeof memoryMethods>,
        ReturnType<typeof colorMethods>,
        ReturnType<typeof filterMethods>,
        ReturnType<typeof modulatorMethods>,
        ReturnType<typeof sourceMethods>,
        ReturnType<typeof warpMethods> {}

/** A one-channel collection as its output, so it shares a tap or control with it. */
const single = (signal: AudioSignal): AudioSignal =>
    signal instanceof BaseCollection && signal.length === 1
        ? signal[0]
        : signal;

export class VideoGraphBuilder implements VideoCore {
    private nodes: VideoNode[] = [];
    private outputId: string | null = null;
    private uniforms: VideoUniform[] = [];
    private previews: VideoPreview[] = [];
    private previewSites: VideoPreviewSite[] = [];
    private cvCount = 0;
    /** The engine tap of each audio signal already published, by signal identity. */
    private tapIndexes = new Map<string, number>();
    private histories: VideoHistory[] = [];
    private sources: VideoSourceDef[] = [];
    /** The audio signal each tap carries, published once `build` knows which taps are live. */
    private tapSignals: AudioSignal[] = [];
    /** The signal each pattern plays as, so a pattern used twice shares one `$cycle`. */
    private played = new Map<object, AudioSignal>();

    constructor(private readonly host: VideoGraphHost) {
        Object.assign(
            this,
            generatorMethods(this),
            mathMethods(this),
            memoryMethods(this),
            colorMethods(this),
            filterMethods(this),
            modulatorMethods(this),
            sourceMethods(this),
            warpMethods(this),
        );
    }

    readonly time = this.signal({ kind: 'time' });

    /** A video signal whose chain methods call this builder's functions. */
    private signal(value: VideoValue): VideoOutput {
        return new VideoOutput(value, this as unknown as VideoOps);
    }

    /**
     * Binds an audio signal to a uniform slot: a slider or button by its
     * backing module, anything else through an engine tap, which sums the
     * channels of a signal that has several. Each source gets one slot however
     * often it is used.
     */
    private bindSignal(
        fn: string,
        name: string,
        signal: AudioSignal,
    ): VideoValue {
        const output = single(signal);
        const control =
            output instanceof ModuleOutput
                ? this.host.controlValue(output.moduleId)
                : undefined;
        if (output instanceof ModuleOutput && control !== undefined) {
            return this.uniformSlot(
                (u) => u.kind === 'control' && u.moduleId === output.moduleId,
                { kind: 'control', moduleId: output.moduleId, value: control },
            );
        }
        const tap = this.tapFor(fn, name, output);
        return this.uniformSlot((u) => u.kind === 'tap' && u.tap === tap, {
            kind: 'tap',
            tap,
            value: 0,
        });
    }

    /** The slot of the uniform `matches` finds, adding `binding` when there is none. */
    private uniformSlot(
        matches: (u: VideoUniform) => boolean,
        binding: DistributiveOmit<VideoUniform, 'slot'>,
    ): VideoValue {
        let uniform = this.uniforms.find(matches);
        if (uniform === undefined) {
            uniform = {
                ...binding,
                slot: this.uniforms.length,
            } as VideoUniform;
            this.uniforms.push(uniform);
        }
        return { kind: 'uniform', slot: uniform.slot };
    }

    /** The engine tap carrying `signal`, summed to one channel, adding it on first use. */
    private tapFor(fn: string, name: string, signal: AudioSignal): number {
        if (signal instanceof BaseCollection && signal.length === 0) {
            throw new Error(`${fn}: ${name} has no channels`);
        }
        const key = [...(signal instanceof BaseCollection ? signal : [signal])]
            .map((o) => `${o.moduleId}\0${o.portName}\0${o.channel}`)
            .join('\n');
        const known = this.tapIndexes.get(key);
        if (known !== undefined) return known;
        if (this.tapSignals.length >= MAX_VIDEO_TAPS) {
            throw new Error(
                `${fn}: ${name} would be signal ${MAX_VIDEO_TAPS + 1}; a patch can read at most ${MAX_VIDEO_TAPS} audio signals into video`,
            );
        }
        const tap = this.tapSignals.push(signal) - 1;
        this.tapIndexes.set(key, tap);
        return tap;
    }

    /** `value` as an audio signal when it is a pattern, playing each pattern once. */
    private play(value: unknown): unknown {
        if (!isPatternValue(value)) return value;
        let signal = this.played.get(value);
        if (signal === undefined) {
            signal = this.host.playPattern(value);
            this.played.set(value, signal);
        }
        return signal;
    }

    value(fn: string, name: string, value: unknown): VideoValue {
        const v = this.play(value);
        if (typeof v === 'number') {
            // The shader holds constants as f32, whose range ends near 3.4e38.
            if (!Number.isFinite(Math.fround(v))) {
                throw new Error(
                    `${fn}: ${name} must be a finite number within ±3.4e38, got ${v}`,
                );
            }
            return { kind: 'const', value: v };
        }
        if (v instanceof VideoOutput) return v.value;
        if (v instanceof ModuleOutput || v instanceof BaseCollection) {
            return this.bindSignal(fn, name, v);
        }
        throw new Error(
            `${fn}: ${name} must be a number, a video signal or an audio signal, got ${describe(v)}`,
        );
    }

    values(fn: string, values: Record<string, unknown>): VideoInputs {
        const inputs: VideoInputs = {};
        for (const [name, v] of Object.entries(values)) {
            inputs[name] = this.value(fn, name, v);
        }
        return inputs;
    }

    addNode(
        kind: string,
        inputs: VideoInputs,
        extra?: VideoNodeExtra,
    ): VideoOutput {
        const id = `${kind}_${this.nodes.length}`;
        this.nodes.push({ id, kind, inputs, ...extra });
        return this.signal({ kind: 'node', id });
    }

    /**
     * The recent audio-rate samples of an audio signal laid along `position`,
     * oldest at 0 and newest at 1, in volts.
     */
    fromAudio = (
        signal: ModuleOutput | BaseCollection<ModuleOutput>,
        position: VideoSource = this.ramp('horizontal'),
        config?: VideoAudioConfig,
    ): VideoOutput => {
        const played = this.play(signal);
        if (
            !(
                played instanceof ModuleOutput ||
                played instanceof BaseCollection
            )
        ) {
            throw new Error(
                `$v.fromAudio: signal must be an audio signal, got ${describe(signal)}`,
            );
        }
        const samples = config?.samples ?? 512;
        if (
            !Number.isInteger(samples) ||
            samples < 2 ||
            samples > VIDEO_HISTORY_LEN
        ) {
            throw new Error(
                `$v.fromAudio: samples must be an integer from 2 to ${VIDEO_HISTORY_LEN}, got ${samples}`,
            );
        }
        const trigger = config?.trigger ?? true;
        const tap = this.tapFor('$v.fromAudio', 'signal', single(played));
        let row = this.histories.findIndex(
            (h) =>
                h.tap === tap && h.samples === samples && h.trigger === trigger,
        );
        if (row < 0) {
            row = this.histories.length;
            this.histories.push({ samples, tap, trigger });
        }
        return this.addNode(
            'audioHistory',
            {
                position: this.value('$v.fromAudio', 'position', position),
                samples: { kind: 'const', value: samples },
            },
            { history: row },
        );
    };

    sourceIndex(def: VideoSourceDef): number {
        const known = this.sources.findIndex((s) => sameSource(s, def));
        if (known >= 0) return known;
        this.sources.push(def);
        return this.sources.length - 1;
    }

    mediaExists(path: string): boolean {
        return this.host.mediaExists?.(path) ?? true;
    }

    mediaAudio(path: string, playback: VideoAudioPlayback): Collection {
        return this.host.mediaAudio(path, playback);
    }

    /** One channel of a signal, or its brightness, as a gray signal. */
    channel = (
        input: VideoSource,
        which: 'r' | 'g' | 'b' | 'a' | 'luma' = 'luma',
    ): VideoOutput =>
        this.addNode(
            'channel',
            { input: this.value('$v.channel', 'input', input) },
            { params: { channel: which } },
        );

    /**
     * Reorders the channels of a signal: `pattern` names, for red, green and
     * blue in turn, the channel of `input` that supplies it, so `'gbr'` makes
     * red from green, green from blue and blue from red, and `'aaa'` shows the
     * alpha. The alpha is kept.
     */
    swiz = (input: VideoSource, pattern: string): VideoOutput => {
        if (typeof pattern !== 'string' || !/^[rgba]{3}$/.test(pattern)) {
            throw new Error(
                `$v.swiz: pattern must be three of r, g, b and a such as 'rrr' or 'gbr', got ${describe(pattern)}`,
            );
        }
        return this.addNode(
            'swizzle',
            { input: this.value('$v.swiz', 'input', input) },
            { params: { pattern } },
        );
    };

    /**
     * Shows `signal` in the editor beside this call and returns it unchanged,
     * so a preview can sit inside an expression.
     */
    preview = (
        signal: VideoOutput,
        config?: VideoPreviewConfig,
    ): VideoOutput => {
        if (!(signal instanceof VideoOutput)) {
            throw new Error(
                `$v.preview: signal must be a video signal, got ${describe(signal)}`,
            );
        }
        const view = config?.view ?? 'image';
        if (!PREVIEW_VIEWS.includes(view)) {
            throw new Error(
                `$v.preview: view must be one of ${PREVIEW_VIEWS.join(', ')}, got "${view}"`,
            );
        }
        this.previewSites.push({
            index: this.previews.length,
            sourceLocation: this.host.sourceLocation(),
            view,
        });
        this.previews.push({ value: signal.value });
        return signal;
    };

    /**
     * Averages a region of `signal` each frame into an audio control signal
     * between 0 and 1. A color contributes its brightness.
     */
    toCV = (
        signal: VideoOutput,
        config?: VideoCvConfig,
    ): CollectionWithRange => {
        if (!(signal instanceof VideoOutput)) {
            throw new Error(
                `$v.toCV: signal must be a video signal, got ${describe(signal)}`,
            );
        }
        const x = config?.x ?? 0.5;
        const y = config?.y ?? 0.5;
        const size = config?.size ?? 0.5;
        for (const [name, value] of [
            ['x', x],
            ['y', y],
            ['size', size],
        ] as const) {
            if (typeof value !== 'number' || !Number.isFinite(value)) {
                throw new Error(
                    `$v.toCV: ${name} must be a finite number, got ${value}`,
                );
            }
        }
        if (size <= 0) {
            throw new Error(
                `$v.toCV: size must be greater than 0, got ${size}`,
            );
        }
        const id = `__videoCV_${this.cvCount++}`;
        this.previews.push({
            cv: { id, size, x, y },
            value: signal.value,
        });
        return this.host.cvSignal(id);
    };

    /** One entry per `$v.preview` call, in the order the shader draws them. */
    getPreviewSites(): VideoPreviewSite[] {
        return this.previewSites;
    }

    /** Shows `input` as the patch's picture and returns it. The last call wins. */
    out = (input: VideoOutput): VideoOutput => {
        this.outputId = this.addOut(this.value('$v.out', 'input', input));
        return input;
    };

    /** Adds the node that shows `input` on screen, and returns its id. */
    private addOut(input: VideoValue): string {
        this.addNode('out', { input });
        return this.nodes[this.nodes.length - 1].id;
    }

    /**
     * The graph reachable from the output and the previews, or null when the
     * patch has neither. A patch with previews but no `$v.out` shows black.
     * Only what that graph uses is kept: the media it opens, the audio it
     * reads, and the taps the engine publishes for it.
     */
    build(): VideoGraph | null {
        const hasOutput = this.outputId !== null;
        if (this.outputId === null && this.previews.length > 0) {
            this.outputId = this.addOut({ kind: 'const', value: 0 });
        }
        if (this.outputId === null) return null;
        const liveNodes = this.liveNodes(this.outputId);

        // Each index the shader binds is renumbered over the live nodes, so
        // what only pruned nodes used leaves no gaps.
        const buffers = keptIndices(liveNodes.map((n) => n.buffer));
        const histories = keptIndices(liveNodes.map((n) => n.history));
        const sources = keptIndices(liveNodes.map((n) => n.source));
        const slots = keptIndices(
            [
                ...liveNodes.flatMap((n) => Object.values(n.inputs)),
                ...this.previews.map((p) => p.value),
            ].map((v) => (v.kind === 'uniform' ? v.slot : undefined)),
        );
        const taps = keptIndices([
            ...slots.map((slot) => {
                const u = this.uniforms[slot];
                return u.kind === 'tap' ? u.tap : undefined;
            }),
            ...histories.map((row) => this.histories[row].tap),
        ]);
        checkTextures(
            buffers.length,
            histories.length > 0 ? 1 : 0,
            sources.length,
        );
        taps.forEach((tap, k) => this.host.publishTap(this.tapSignals[tap], k));

        const remap = (v: VideoValue): VideoValue =>
            v.kind === 'uniform' ? { ...v, slot: slots.indexOf(v.slot) } : v;
        return {
            nodes: liveNodes.map((n) => ({
                ...n,
                inputs: Object.fromEntries(
                    Object.entries(n.inputs).map(([k, v]) => [k, remap(v)]),
                ),
                ...(n.buffer !== undefined && {
                    buffer: buffers.indexOf(n.buffer),
                }),
                ...(n.history !== undefined && {
                    history: histories.indexOf(n.history),
                }),
                ...(n.source !== undefined && {
                    source: sources.indexOf(n.source),
                }),
            })),
            output: this.outputId,
            hasOutput,
            histories: histories.map((row) => ({
                ...this.histories[row],
                tap: taps.indexOf(this.histories[row].tap),
            })),
            sources: sources.map((k) => this.sources[k]),
            previews: this.previews.map((p) => ({
                ...p,
                value: remap(p.value),
            })),
            uniforms: slots.map((slot, k) => {
                const u = this.uniforms[slot];
                return u.kind === 'tap'
                    ? { ...u, slot: k, tap: taps.indexOf(u.tap) }
                    : { ...u, slot: k };
            }),
        };
    }

    /** The nodes the output, the previews and the buffer writes read, in graph order. */
    private liveNodes(outputId: string): VideoNode[] {
        const live = new Set<string>(
            this.nodes
                .filter((n) => n.id === outputId || n.kind === 'feedbackWrite')
                .map((n) => n.id),
        );
        for (const preview of this.previews) {
            if (preview.value.kind === 'node') live.add(preview.value.id);
        }
        for (let i = this.nodes.length - 1; i >= 0; i--) {
            const node = this.nodes[i];
            if (!live.has(node.id)) continue;
            for (const input of Object.values(node.inputs)) {
                if (input.kind === 'node') live.add(input.id);
            }
        }
        const liveNodes = this.nodes.filter((n) => live.has(n.id));
        const written = new Set(
            liveNodes.flatMap((n) =>
                n.kind === 'feedbackWrite' ? [n.buffer] : [],
            ),
        );
        if (
            liveNodes.some(
                (n) => n.kind === 'feedbackRead' && !written.has(n.buffer),
            )
        ) {
            throw new Error(
                '$v.buffer: a buffer is read but never written; give it a signal with .write(...)',
            );
        }
        return liveNodes;
    }
}

/** The distinct indices in `indices`, ascending. */
function keptIndices(indices: (number | undefined)[]): number[] {
    return [
        ...new Set(indices.filter((i): i is number => i !== undefined)),
    ].sort((a, b) => a - b);
}

/** Rejects a graph that samples more textures than WebGPU guarantees a shader. */
function checkTextures(
    buffers: number,
    histories: number,
    sources: number,
): void {
    if (buffers + histories + sources <= MAX_SAMPLED_TEXTURES) return;
    const used = (
        [
            [buffers, 'feedback buffer'],
            [histories, 'audio history'],
            [sources, 'source'],
        ] as const
    )
        .filter(([count]) => count > 0)
        .map(([count, what]) => `${count} ${what}${count === 1 ? '' : 's'}`);
    const listed = `${used.slice(0, -1).join(', ')} and ${used[used.length - 1]}`;
    throw new Error(
        `$v: a patch can use at most ${MAX_SAMPLED_TEXTURES} textures, counting feedback buffers and frame delays, one for all of $v.fromAudio, and one per media source; this one uses ${listed}`,
    );
}
