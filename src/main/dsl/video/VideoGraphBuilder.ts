import {
    MAX_FEEDBACK_BUFFERS,
    MAX_VIDEO_TAPS,
    type VideoGraph,
    type VideoNode,
    type VideoPreview,
    type VideoUniform,
    type VideoValue,
    type VideoValueType,
} from '../../../shared/video/videoGraph';
import { BaseCollection, ModuleOutput } from '../GraphBuilder';

/** A video signal: a node's output, a constant, or the time field. */
export class VideoOutput {
    constructor(
        readonly value: VideoValue,
        readonly type: VideoValueType,
    ) {}
}

/**
 * A constant, a video signal of either type, or an audio signal (a slider,
 * button or any module output), whose live value drives a field input.
 */
export type VideoSource =
    | number
    | VideoOutput
    | ModuleOutput
    | BaseCollection<ModuleOutput>;

/** What the builder needs from the patch it is building inside. */
export interface VideoGraphHost {
    /**
     * Current value of a slider or button's backing module, or undefined if
     * `moduleId` is not a control.
     */
    controlValue(moduleId: string): number | undefined;
    /** Publishes `output` to tap slot `slot`, as the engine's `_videoTap` module. */
    publishTap(output: ModuleOutput, slot: number): void;
    /** Where the patch script is calling from, as V8 reports it. */
    sourceLocation(): { line: number; column: number } | undefined;
}

export type VideoPreviewView = 'image' | 'waveform' | 'vectorscope';

export interface VideoPreviewConfig {
    /** How the editor draws the signal (default 'image'). */
    view?: VideoPreviewView;
}

/** Editor-side description of one `$v.preview` call. */
export interface VideoPreviewSite {
    view: VideoPreviewView;
    sourceLocation?: { line: number; column: number };
}

const PREVIEW_VIEWS: readonly string[] = ['image', 'waveform', 'vectorscope'];

export interface VideoOscConfig {
    shape?: 'sine' | 'triangle' | 'saw' | 'square';
}

export interface VideoRampConfig {
    /** Magnification about the center (default 1). */
    zoom?: VideoSource;
    /** Turns; positive turns the pattern clockwise (default 0). */
    rotate?: VideoSource;
    /** Horizontal move, as a fraction of frame width (default 0). */
    shiftX?: VideoSource;
    /** Vertical move, as a fraction of frame height (default 0). */
    shiftY?: VideoSource;
}

export interface VideoFeedbackConfig {
    /** Magnification of the previous frame about the center (default 1). */
    zoom?: VideoSource;
    /** Turns per frame (default 0). */
    rotate?: VideoSource;
    /** Horizontal move, as a fraction of frame width per frame (default 0). */
    shiftX?: VideoSource;
    /** Vertical move, as a fraction of frame height per frame (default 0). */
    shiftY?: VideoSource;
    /** What lies beyond the frame border (default 'clamp'). */
    edge?: 'clamp' | 'repeat' | 'mirror';
}

export interface VideoShapeConfig {
    shape?: 'circle' | 'box' | 'diamond';
}

function describe(value: unknown): string {
    if (value instanceof VideoOutput) return `a ${value.type}`;
    if (value instanceof ModuleOutput || value instanceof BaseCollection) {
        return 'an audio signal';
    }
    return String(value);
}

const isColor = (value: unknown): boolean =>
    value instanceof VideoOutput && value.type === 'color';

/** Collects `$v.*` calls into a {@link VideoGraph}. */
export class VideoGraphBuilder {
    private nodes: VideoNode[] = [];
    private outputId: string | null = null;
    private uniforms: VideoUniform[] = [];
    private bufferCount = 0;
    private previews: VideoPreview[] = [];
    private previewSites: VideoPreviewSite[] = [];
    /** Uniform slot of each audio signal already published, by signal identity. */
    private tapSlots = new Map<string, number>();
    private tapCount = 0;

    constructor(private readonly host: VideoGraphHost) {}

    readonly time = new VideoOutput({ kind: 'time' }, 'field');

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

        const key = `${output.moduleId}\0${output.portName}\0${output.channel}`;
        const known = this.tapSlots.get(key);
        if (known !== undefined) return { kind: 'uniform', slot: known };
        if (this.tapCount >= MAX_VIDEO_TAPS) {
            throw new Error(
                `${fn}: ${name} would be signal ${MAX_VIDEO_TAPS + 1}; a patch can read at most ${MAX_VIDEO_TAPS} audio signals into video`,
            );
        }
        const tap = this.tapCount++;
        this.host.publishTap(output, tap);
        const slot = this.uniforms.length;
        this.uniforms.push({ kind: 'tap', slot, tap, value: 0 });
        this.tapSlots.set(key, slot);
        return { kind: 'uniform', slot };
    }

    private asField(fn: string, name: string, v: unknown): VideoValue {
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

    private asColor(fn: string, name: string, v: unknown): VideoValue {
        if (v instanceof VideoOutput && v.type === 'color') return v.value;
        throw new Error(
            `${fn}: ${name} must be a video color, got ${describe(v)}`,
        );
    }

    /** A color operand; a field or number becomes the gray of that level. */
    private asColorOrGray(fn: string, name: string, v: unknown): VideoValue {
        if (isColor(v)) return this.asColor(fn, name, v);
        const level = this.asField(fn, name, v);
        return this.addNode('colorize', 'color', {
            r: level,
            g: level,
            b: level,
        }).value;
    }

    private addNode(
        kind: string,
        type: VideoValueType,
        inputs: Record<string, VideoValue>,
        params?: Record<string, string>,
        buffer?: number,
    ): VideoOutput {
        const id = `${kind}_${this.nodes.length}`;
        this.nodes.push({ id, kind, inputs, params, buffer });
        return new VideoOutput({ kind: 'node', id }, type);
    }

    /**
     * Builds a math node whose operands are fields, or colors when any operand
     * is a color (`<kind>Color`). `fieldInputs` are fields in both variants.
     */
    private arith(
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
     * Scan ramp over the frame, optionally zoomed, rotated and shifted. `h` and
     * `v` run 0 to 1 across and up, `d` along the diagonal, `r` is the distance
     * from the center and `a` the angle around it.
     */
    ramp = (
        axis: 'h' | 'v' | 'd' | 'r' | 'a' = 'h',
        config?: VideoRampConfig,
    ): VideoOutput =>
        this.addNode(
            'ramp',
            'field',
            {
                zoom: this.asField('$v.ramp', 'zoom', config?.zoom ?? 1),
                rotate: this.asField('$v.ramp', 'rotate', config?.rotate ?? 0),
                shiftX: this.asField('$v.ramp', 'shiftX', config?.shiftX ?? 0),
                shiftY: this.asField('$v.ramp', 'shiftY', config?.shiftY ?? 0),
            },
            { axis },
        );

    /** Periodic shaper: `freq` cycles per unit of `input`, offset by `phase` cycles. */
    osc = (
        input: VideoSource,
        freq: VideoSource,
        phase: VideoSource = 0,
        config?: VideoOscConfig,
    ): VideoOutput =>
        this.addNode(
            'osc',
            'field',
            {
                input: this.asField('$v.osc', 'input', input),
                freq: this.asField('$v.osc', 'freq', freq),
                phase: this.asField('$v.osc', 'phase', phase),
            },
            config?.shape === undefined ? undefined : { shape: config.shape },
        );

    /** 1 inside a shape centered on (x, y), 0 outside. */
    shape = (
        x: VideoSource,
        y: VideoSource,
        size: VideoSource = 0.25,
        softness: VideoSource = 0.01,
        config?: VideoShapeConfig,
    ): VideoOutput =>
        this.addNode(
            'shape',
            'field',
            {
                x: this.asField('$v.shape', 'x', x),
                y: this.asField('$v.shape', 'y', y),
                size: this.asField('$v.shape', 'size', size),
                softness: this.asField('$v.shape', 'softness', softness),
            },
            config?.shape === undefined ? undefined : { shape: config.shape },
        );

    /** Sum, clipped to 0..1. Colors add per channel. */
    add = (a: VideoSource, b: VideoSource): VideoOutput =>
        this.arith('$v.add', 'add', { a, b });

    /** Product. Colors multiply per channel. */
    mult = (a: VideoSource, b: VideoSource): VideoOutput =>
        this.arith('$v.mult', 'mult', { a, b });

    /** Absolute difference. Colors differ per channel. */
    diff = (a: VideoSource, b: VideoSource): VideoOutput =>
        this.arith('$v.diff', 'diff', { a, b });

    /** The larger of two values; colors take it per channel. */
    max = (a: VideoSource, b: VideoSource): VideoOutput =>
        this.arith('$v.max', 'max', { a, b });

    /** The smaller of two values; colors take it per channel. */
    min = (a: VideoSource, b: VideoSource): VideoOutput =>
        this.arith('$v.min', 'min', { a, b });

    /** Multiplies by `gain`, then keeps the fractional part. */
    wrap = (input: VideoSource, gain: VideoSource = 1): VideoOutput =>
        this.addNode('wrap', 'field', {
            input: this.asField('$v.wrap', 'input', input),
            gain: this.asField('$v.wrap', 'gain', gain),
        });

    /** Multiplies by `gain`, then reflects whatever passes 1 back down. */
    fold = (input: VideoSource, gain: VideoSource = 1): VideoOutput =>
        this.addNode('fold', 'field', {
            input: this.asField('$v.fold', 'input', input),
            gain: this.asField('$v.fold', 'gain', gain),
        });

    /** Complement: 1 - input. */
    invert = (input: VideoSource): VideoOutput =>
        this.arith('$v.invert', 'invert', { input });

    /** Crossfade from `a` (amount 0) to `b` (amount 1). */
    mix = (
        a: VideoSource,
        b: VideoSource,
        amount: VideoSource = 0.5,
    ): VideoOutput => this.arith('$v.mix', 'mix', { a, b }, { amount });

    /** Threshold: 0 below `threshold`, 1 above, with a ramp `softness` wide. */
    comparator = (
        input: VideoSource,
        threshold: VideoSource = 0.5,
        softness: VideoSource = 0,
    ): VideoOutput =>
        this.addNode('comparator', 'field', {
            input: this.asField('$v.comparator', 'input', input),
            threshold: this.asField('$v.comparator', 'threshold', threshold),
            softness: this.asField('$v.comparator', 'softness', softness),
        });

    /** Shows `fg` where `mask` is 1 and `bg` where it is 0. */
    key = (fg: VideoSource, bg: VideoSource, mask: VideoSource): VideoOutput =>
        this.addNode('key', 'color', {
            fg: this.asColorOrGray('$v.key', 'fg', fg),
            bg: this.asColorOrGray('$v.key', 'bg', bg),
            mask: this.asField('$v.key', 'mask', mask),
        });

    /** Quantizes to `levels` values between 0 and 1. */
    posterize = (input: VideoSource, levels: VideoSource = 4): VideoOutput =>
        this.addNode('posterize', 'field', {
            input: this.asField('$v.posterize', 'input', input),
            levels: this.asField('$v.posterize', 'levels', levels),
        });

    /** Combines three fields into a color. */
    colorize = (r: VideoSource, g: VideoSource, b: VideoSource): VideoOutput =>
        this.addNode('colorize', 'color', {
            r: this.asField('$v.colorize', 'r', r),
            g: this.asField('$v.colorize', 'g', g),
            b: this.asField('$v.colorize', 'b', b),
        });

    /** Color from hue (wraps every 1.0), saturation and value. */
    hsv = (
        h: VideoSource,
        s: VideoSource = 1,
        v: VideoSource = 1,
    ): VideoOutput =>
        this.addNode('hsv', 'color', {
            h: this.asField('$v.hsv', 'h', h),
            s: this.asField('$v.hsv', 's', s),
            v: this.asField('$v.hsv', 'v', v),
        });

    /** Saturation, then gain and bias, clipped to the displayable range. */
    procAmp = (
        input: VideoSource,
        gain: VideoSource = 1,
        bias: VideoSource = 0,
        saturation: VideoSource = 1,
    ): VideoOutput =>
        this.addNode('procAmp', 'color', {
            input: this.asColorOrGray('$v.procAmp', 'input', input),
            gain: this.asField('$v.procAmp', 'gain', gain),
            bias: this.asField('$v.procAmp', 'bias', bias),
            saturation: this.asField('$v.procAmp', 'saturation', saturation),
        });

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
        if (this.bufferCount >= MAX_FEEDBACK_BUFFERS) {
            throw new Error(
                `$v.feedback: a patch can use at most ${MAX_FEEDBACK_BUFFERS} feedback loops`,
            );
        }
        const buffer = this.bufferCount++;
        const prev = this.addNode(
            'feedbackRead',
            'color',
            {
                zoom: this.asField('$v.feedback', 'zoom', config?.zoom ?? 1),
                rotate: this.asField(
                    '$v.feedback',
                    'rotate',
                    config?.rotate ?? 0,
                ),
                shiftX: this.asField(
                    '$v.feedback',
                    'shiftX',
                    config?.shiftX ?? 0,
                ),
                shiftY: this.asField(
                    '$v.feedback',
                    'shiftY',
                    config?.shiftY ?? 0,
                ),
            },
            config?.edge === undefined ? undefined : { edge: config.edge },
            buffer,
        );
        const next = update(prev);
        if (!(next instanceof VideoOutput) || next.type !== 'color') {
            throw new Error(
                `$v.feedback: update must return a video color, got ${describe(next)}`,
            );
        }
        this.addNode(
            'feedbackWrite',
            'color',
            { input: next.value },
            undefined,
            buffer,
        );
        return next;
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
        this.previews.push({ type: signal.type, value: signal.value });
        this.previewSites.push({
            sourceLocation: this.host.sourceLocation(),
            view,
        });
        return signal;
    };

    /** One entry per `$v.preview` call, in the order the shader draws them. */
    getPreviewSites(): VideoPreviewSite[] {
        return this.previewSites;
    }

    /** Shows `input` in the performance window. The last call wins. */
    out = (input: VideoOutput): void => {
        if (!(input instanceof VideoOutput) || input.type !== 'color') {
            throw new Error(
                `$v.out: input must be a video color, got ${describe(input)}`,
            );
        }
        this.addNode('out', 'color', { input: input.value });
        this.outputId = this.nodes[this.nodes.length - 1].id;
    };

    /**
     * The graph reachable from the output and the previews, or null when the
     * patch has neither. A patch with previews but no `$v.out` shows black.
     */
    build(): VideoGraph | null {
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
        return {
            nodes: this.nodes.filter((n) => live.has(n.id)),
            output: this.outputId,
            previews: this.previews,
            uniforms: this.uniforms,
        };
    }
}
