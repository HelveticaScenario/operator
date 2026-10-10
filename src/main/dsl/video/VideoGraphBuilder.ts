import {
    MAX_FEEDBACK_BUFFERS,
    MAX_VIDEO_TAPS,
    type VideoGraph,
    type VideoHistory,
    VIDEO_HISTORY_LEN,
    type VideoNode,
    type VideoPreview,
    type VideoPreviewSite,
    type VideoSourceDef,
    type VideoUniform,
    type VideoValue,
    type VideoValueType,
} from '../../../shared/video/videoGraph';
import {
    BaseCollection,
    type CollectionWithRange,
    ModuleOutput,
} from '../GraphBuilder';
import { VideoBuffer } from './VideoBuffer';
import { VideoOutput } from './VideoOutput';
import { colorMethods } from './videoColor';
import { filterMethods } from './videoFilters';
import { generatorMethods } from './videoGenerators';
import { mathMethods } from './videoMath';
import { sourceMethods } from './videoSources';
import { warpMethods } from './videoWarps';
import {
    describe,
    isColor,
    type VideoAudioConfig,
    type VideoCore,
    type VideoCvConfig,
    type VideoFeedbackConfig,
    type VideoGraphHost,
    type VideoPreviewConfig,
    type VideoSource,
} from './videoBuilderTypes';

const PREVIEW_VIEWS: readonly string[] = ['image', 'waveform', 'vectorscope'];

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
        ReturnType<typeof colorMethods>,
        ReturnType<typeof filterMethods>,
        ReturnType<typeof sourceMethods>,
        ReturnType<typeof warpMethods> {}

export class VideoGraphBuilder implements VideoCore {
    private nodes: VideoNode[] = [];
    private outputId: string | null = null;
    private uniforms: VideoUniform[] = [];
    private bufferCount = 0;
    private writtenBuffers = new Set<number>();
    private previews: VideoPreview[] = [];
    private previewSites: VideoPreviewSite[] = [];
    private cvCount = 0;
    /** Uniform slot of each audio signal already published, by signal identity. */
    private tapSlots = new Map<string, number>();
    private tapIndexes = new Map<string, number>();
    private histories: VideoHistory[] = [];
    private sources: VideoSourceDef[] = [];
    private tapCount = 0;

    constructor(private readonly host: VideoGraphHost) {
        Object.assign(
            this,
            generatorMethods(this),
            mathMethods(this),
            colorMethods(this),
            filterMethods(this),
            sourceMethods(this),
            warpMethods(this),
        );
    }

    readonly time = new VideoOutput({ kind: 'time' }, 'field', this);

    /**
     * Binds an audio signal to a uniform slot: a slider or button by its
     * backing module, anything else through an engine tap. Each source gets
     * one slot however often it is used.
     */
    private bindSignal(
        fn: string,
        name: string,
        output: ModuleOutput,
    ): VideoValue {
        const control = this.host.controlValue(output.moduleId);
        if (control !== undefined) {
            const existing = this.uniforms.find(
                (u) => u.kind === 'control' && u.moduleId === output.moduleId,
            );
            if (existing) return { kind: 'uniform', slot: existing.slot };
            const slot = this.uniforms.length;
            this.uniforms.push({
                kind: 'control',
                moduleId: output.moduleId,
                slot,
                value: control,
            });
            return { kind: 'uniform', slot };
        }

        const key = this.signalKey(output);
        const known = this.tapSlots.get(key);
        if (known !== undefined) return { kind: 'uniform', slot: known };
        const tap = this.tapFor(fn, name, output);
        const slot = this.uniforms.length;
        this.uniforms.push({ kind: 'tap', slot, tap, value: 0 });
        this.tapSlots.set(key, slot);
        return { kind: 'uniform', slot };
    }

    private signalKey(output: ModuleOutput): string {
        return `${output.moduleId}\0${output.portName}\0${output.channel}`;
    }

    /** The engine tap carrying `output`, publishing it on first use. */
    private tapFor(fn: string, name: string, output: ModuleOutput): number {
        const key = this.signalKey(output);
        const known = this.tapIndexes.get(key);
        if (known !== undefined) return known;
        if (this.tapCount >= MAX_VIDEO_TAPS) {
            throw new Error(
                `${fn}: ${name} would be signal ${MAX_VIDEO_TAPS + 1}; a patch can read at most ${MAX_VIDEO_TAPS} audio signals into video`,
            );
        }
        const tap = this.tapCount++;
        this.host.publishTap(output, tap);
        this.tapIndexes.set(key, tap);
        return tap;
    }

    asField(fn: string, name: string, v: unknown): VideoValue {
        if (typeof v === 'number') return { kind: 'const', value: v };
        if (v instanceof VideoOutput && v.type === 'field') return v.value;
        if (v instanceof ModuleOutput) return this.bindSignal(fn, name, v);
        if (v instanceof BaseCollection && v.length === 1) {
            return this.bindSignal(fn, name, v[0]);
        }
        if (v instanceof BaseCollection) {
            throw new Error(
                `${fn}: ${name} has ${v.length} channels; video inputs take one, so pick a channel such as signal[0]`,
            );
        }
        throw new Error(
            `${fn}: ${name} must be a number or a video field, got ${describe(v)}`,
        );
    }

    /** A color operand; a field or number becomes the gray of that level. */
    asColorOrGray(fn: string, name: string, v: unknown): VideoValue {
        return this.toColor(fn, name, v).value;
    }

    /** `v` as a color signal; a field or number becomes the gray of that level. */
    toColor(fn: string, name: string, v: unknown): VideoOutput {
        if (v instanceof VideoOutput && v.type === 'color') return v;
        const level = this.asField(fn, name, v);
        return this.addNode('colorize', 'color', {
            r: level,
            g: level,
            b: level,
        });
    }

    addNode(
        kind: string,
        type: VideoValueType,
        inputs: Record<string, VideoValue>,
        params?: Record<string, string>,
        buffer?: number,
        history?: number,
        source?: number,
    ): VideoOutput {
        const id = `${kind}_${this.nodes.length}`;
        this.nodes.push({ id, kind, inputs, params, buffer, history, source });
        return new VideoOutput({ kind: 'node', id }, type, this);
    }

    /**
     * Builds a math node whose operands are fields, or colors when any operand
     * is a color (`<kind>Color`). `fieldInputs` are fields in both variants.
     */
    arith(
        fn: string,
        kind: string,
        operands: Record<string, unknown>,
        fieldInputs: Record<string, unknown> = {},
    ): VideoOutput {
        const color = Object.values(operands).some(isColor);
        const inputs: Record<string, VideoValue> = {};
        for (const [name, v] of Object.entries(operands)) {
            inputs[name] = color
                ? this.asColorOrGray(fn, name, v)
                : this.asField(fn, name, v);
        }
        for (const [name, v] of Object.entries(fieldInputs)) {
            inputs[name] = this.asField(fn, name, v);
        }
        return this.addNode(
            color ? `${kind}Color` : kind,
            color ? 'color' : 'field',
            inputs,
        );
    }

    /**
     * The recent audio-rate samples of an audio signal laid along `position`,
     * oldest at 0 and newest at 1, in volts.
     */
    fromAudio = (
        signal: ModuleOutput | BaseCollection<ModuleOutput>,
        position: VideoSource = this.ramp('h'),
        config?: VideoAudioConfig,
    ): VideoOutput => {
        const output =
            signal instanceof BaseCollection && signal.length === 1
                ? signal[0]
                : signal;
        if (!(output instanceof ModuleOutput)) {
            throw new Error(
                `$v.fromAudio: signal must be a single-channel audio signal, got ${describe(signal)}`,
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
        const tap = this.tapFor('$v.fromAudio', 'signal', output);
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
            'field',
            {
                position: this.asField('$v.fromAudio', 'position', position),
                samples: { kind: 'const', value: samples },
            },
            undefined,
            undefined,
            row,
        );
    };
    sourceIndex(def: VideoSourceDef): number {
        const known = this.sources.findIndex(
            (s) =>
                s.kind === def.kind &&
                s.path === def.path &&
                s.speed === def.speed &&
                s.loopStart === def.loopStart &&
                s.loopEnd === def.loopEnd,
        );
        if (known >= 0) return known;
        this.sources.push(def);
        return this.sources.length - 1;
    }

    mediaExists(path: string): boolean {
        return this.host.mediaExists?.(path) ?? true;
    }

    /** One channel of a color as a field; every channel of a field is the field itself. */
    channel = (
        input: VideoOutput,
        which: 'r' | 'g' | 'b' | 'luma' = 'luma',
    ): VideoOutput => {
        if (!(input instanceof VideoOutput)) {
            throw new Error(
                `$v.channel: input must be a video field or color, got ${describe(input)}`,
            );
        }
        if (input.type === 'field') return input;
        return this.addNode(
            'channel',
            'field',
            { input: input.value },
            { channel: which },
        );
    };

    /**
     * Reorders the channels of a color: `pattern` names, for red, green and
     * blue in turn, the channel of `input` that supplies it, so `'gbr'` makes
     * red from green, green from blue and blue from red. A field is gray, so
     * every pattern gives it back as a color.
     */
    swiz = (input: VideoOutput, pattern: string): VideoOutput => {
        if (typeof pattern !== 'string' || !/^[rgb]{3}$/.test(pattern)) {
            throw new Error(
                `$v.swiz: pattern must be three of r, g and b such as 'rrr' or 'gbr', got ${describe(pattern)}`,
            );
        }
        return this.addNode(
            'swizzle',
            'color',
            { input: this.toColor('$v.swiz', 'input', input).value },
            { pattern },
        );
    };

    /**
     * Feeds a frame back into itself. `update` receives the previous frame's
     * result, resampled through the transform in `config`, and returns this
     * frame's color; `feedback` returns that color.
     */
    feedback = (
        update: (prev: VideoOutput) => VideoOutput,
        config?: VideoFeedbackConfig,
    ): VideoOutput => {
        if (typeof update !== 'function') {
            throw new Error(
                `$v.feedback: update must be a function, got ${describe(update)}`,
            );
        }
        const index = this.allocateBuffer('$v.feedback');
        const next = update(this.readBuffer(index, config, '$v.feedback'));
        if (!(next instanceof VideoOutput)) {
            throw new Error(
                `$v.feedback: update must return a video field or color, got ${describe(next)}`,
            );
        }
        return this.writeBuffer(index, next, '$v.feedback');
    };

    /**
     * A frame store: signals write it, and any signal can read what it held on
     * the previous frame, so buffers can feed themselves or each other.
     */
    buffer = (): VideoBuffer =>
        new VideoBuffer(this.allocateBuffer('$v.buffer'), this);

    private allocateBuffer(fn: string): number {
        if (this.bufferCount >= MAX_FEEDBACK_BUFFERS) {
            throw new Error(
                `${fn}: a patch can use at most ${MAX_FEEDBACK_BUFFERS} feedback loops`,
            );
        }
        return this.bufferCount++;
    }

    /** The previous frame of buffer `index`, resampled through the transform in `config`. */
    readBuffer = (
        index: number,
        config?: VideoFeedbackConfig,
        fn = '$v.buffer',
    ): VideoOutput =>
        this.addNode(
            'feedbackRead',
            'color',
            {
                zoom: this.asField(fn, 'zoom', config?.zoom ?? 1),
                rotate: this.asField(fn, 'rotate', config?.rotate ?? 0),
                shiftX: this.asField(fn, 'shiftX', config?.shiftX ?? 0),
                shiftY: this.asField(fn, 'shiftY', config?.shiftY ?? 0),
            },
            config?.edge === undefined ? undefined : { edge: config.edge },
            index,
        );

    /** Stores `input` in buffer `index` for the next frame, and returns it. */
    writeBuffer = (
        index: number,
        input: VideoOutput,
        fn = '$v.buffer',
    ): VideoOutput => {
        if (!(input instanceof VideoOutput)) {
            throw new Error(
                `${fn}: write takes a video field or color, got ${describe(input)}`,
            );
        }
        const color = this.toColor(fn, 'input', input);
        if (this.writtenBuffers.has(index)) {
            throw new Error(`${fn}: a buffer can be written only once`);
        }
        this.writtenBuffers.add(index);
        this.addNode(
            'feedbackWrite',
            'color',
            { input: color.value },
            undefined,
            index,
        );
        return color;
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
                `$v.preview: signal must be a video field or color, got ${describe(signal)}`,
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
        this.previews.push({ type: signal.type, value: signal.value });
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
                `$v.toCV: signal must be a video field or color, got ${describe(signal)}`,
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
            type: signal.type,
            value: signal.value,
        });
        return this.host.cvSignal(id);
    };

    /** One entry per `$v.preview` call, in the order the shader draws them. */
    getPreviewSites(): VideoPreviewSite[] {
        return this.previewSites;
    }

    /** Shows `input` as the patch's picture. The last call wins. */
    out = (input: VideoOutput): void => {
        this.addNode('out', 'color', {
            input: this.asColorOrGray('$v.out', 'input', input),
        });
        this.outputId = this.nodes[this.nodes.length - 1].id;
    };

    /**
     * The graph reachable from the output and the previews, or null when the
     * patch has neither. A patch with previews but no `$v.out` shows black.
     */
    build(): VideoGraph | null {
        const hasOutput = this.outputId !== null;
        if (this.outputId === null && this.previews.length > 0) {
            const black = { kind: 'const', value: 0 } as const;
            const color = this.addNode('colorize', 'color', {
                b: black,
                g: black,
                r: black,
            });
            this.addNode('out', 'color', { input: color.value });
            this.outputId = this.nodes[this.nodes.length - 1].id;
        }
        if (this.outputId === null) return null;
        const live = new Set<string>(
            this.nodes
                .filter(
                    (n) => n.id === this.outputId || n.kind === 'feedbackWrite',
                )
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
        // Buffers a patch allocated but never used, or whose reads were all
        // pruned, must not leave gaps in the numbering the shader binds.
        const used = [
            ...new Set(
                liveNodes.flatMap((n) =>
                    n.buffer === undefined ? [] : [n.buffer],
                ),
            ),
        ].sort((a, b) => a - b);
        return {
            nodes: liveNodes.map((n) =>
                n.buffer === undefined
                    ? n
                    : { ...n, buffer: used.indexOf(n.buffer) },
            ),
            output: this.outputId,
            hasOutput,
            histories: this.histories,
            sources: this.sources,
            previews: this.previews,
            uniforms: this.uniforms,
        };
    }
}
