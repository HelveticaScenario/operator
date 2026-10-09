import type {
    VideoGraph,
    VideoNode,
    VideoUniform,
    VideoValue,
    VideoValueType,
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
 * A constant, a video signal of either type, or a slider or button, whose
 * live value drives a field input.
 */
export type VideoSource =
    | number
    | VideoOutput
    | ModuleOutput
    | BaseCollection<ModuleOutput>;

export interface VideoOscConfig {
    shape?: 'sine' | 'triangle' | 'saw' | 'square';
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

    /**
     * @param controlValue Current value of a slider or button's backing
     *   module, or undefined if `moduleId` is not a control.
     */
    constructor(
        private readonly controlValue: (moduleId: string) => number | undefined,
    ) {}

    readonly time = new VideoOutput({ kind: 'time' }, 'field');

    /** Binds a slider or button to a uniform slot, one slot per control. */
    private bindControl(
        fn: string,
        name: string,
        output: ModuleOutput,
    ): VideoValue {
        const existing = this.uniforms.find(
            (u) => u.moduleId === output.moduleId,
        );
        if (existing) return { kind: 'uniform', slot: existing.slot };
        const value = this.controlValue(output.moduleId);
        if (value === undefined) {
            throw new Error(
                `${fn}: ${name} is ${describe(output)}; video inputs take numbers, video signals, sliders and buttons`,
            );
        }
        const slot = this.uniforms.length;
        this.uniforms.push({ slot, moduleId: output.moduleId, value });
        return { kind: 'uniform', slot };
    }

    private asField(fn: string, name: string, v: unknown): VideoValue {
        if (typeof v === 'number') return { kind: 'const', value: v };
        if (v instanceof VideoOutput && v.type === 'field') return v.value;
        if (v instanceof ModuleOutput) return this.bindControl(fn, name, v);
        if (v instanceof BaseCollection && v.length === 1) {
            return this.bindControl(fn, name, v[0]);
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
    ): VideoOutput {
        const id = `${kind}_${this.nodes.length}`;
        this.nodes.push({ id, kind, inputs, params });
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

    /** Scan ramp from 0 to 1 along the horizontal, vertical or diagonal axis. */
    ramp = (axis: 'h' | 'v' | 'd' = 'h'): VideoOutput =>
        this.addNode('ramp', 'field', {}, { axis });

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

    /** The graph reachable from the output, or null if `$v.out` was never called. */
    build(): VideoGraph | null {
        if (this.outputId === null) return null;
        const live = new Set<string>([this.outputId]);
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
            uniforms: this.uniforms,
        };
    }
}
