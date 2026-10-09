import type {
    VideoGraph,
    VideoNode,
    VideoValue,
    VideoValueType,
} from '../../../shared/video/videoGraph';

/** A video signal: a node's output, a constant, or the time field. */
export class VideoOutput {
    constructor(
        readonly value: VideoValue,
        readonly type: VideoValueType,
    ) {}
}

export type VideoField = number | VideoOutput;

export interface VideoOscConfig {
    shape?: 'sine' | 'triangle' | 'saw' | 'square';
}

function describe(value: unknown): string {
    return value instanceof VideoOutput ? `a ${value.type}` : String(value);
}

/** Collects `$v.*` calls into a {@link VideoGraph}. */
export class VideoGraphBuilder {
    private nodes: VideoNode[] = [];
    private outputId: string | null = null;

    readonly time = new VideoOutput({ kind: 'time' }, 'field');

    private asField(fn: string, name: string, v: unknown): VideoValue {
        if (typeof v === 'number') return { kind: 'const', value: v };
        if (v instanceof VideoOutput && v.type === 'field') return v.value;
        throw new Error(
            `${fn}: ${name} must be a number or a video field, got ${describe(v)}`,
        );
    }

    private add(
        kind: string,
        type: VideoValueType,
        inputs: Record<string, VideoValue>,
        params?: Record<string, string>,
    ): VideoOutput {
        const id = `${kind}_${this.nodes.length}`;
        this.nodes.push({ id, kind, inputs, params });
        return new VideoOutput({ kind: 'node', id }, type);
    }

    /** Scan ramp from 0 to 1 along the horizontal, vertical or diagonal axis. */
    ramp = (axis: 'h' | 'v' | 'd' = 'h'): VideoOutput =>
        this.add('ramp', 'field', {}, { axis });

    /** Periodic shaper: `freq` cycles per unit of `input`, offset by `phase` cycles. */
    osc = (
        input: VideoField,
        freq: VideoField,
        phase: VideoField = 0,
        config?: VideoOscConfig,
    ): VideoOutput =>
        this.add(
            'osc',
            'field',
            {
                input: this.asField('$v.osc', 'input', input),
                freq: this.asField('$v.osc', 'freq', freq),
                phase: this.asField('$v.osc', 'phase', phase),
            },
            config?.shape === undefined ? undefined : { shape: config.shape },
        );

    /** Combines three fields into a color. */
    colorize = (r: VideoField, g: VideoField, b: VideoField): VideoOutput =>
        this.add('colorize', 'color', {
            r: this.asField('$v.colorize', 'r', r),
            g: this.asField('$v.colorize', 'g', g),
            b: this.asField('$v.colorize', 'b', b),
        });

    /** Shows `input` in the video output window. The last call wins. */
    out = (input: VideoOutput): void => {
        if (!(input instanceof VideoOutput) || input.type !== 'color') {
            throw new Error(
                `$v.out: input must be a video color, got ${describe(input)}`,
            );
        }
        this.add('out', 'color', { input: input.value });
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
            uniformSlotCount: 0,
        };
    }
}
