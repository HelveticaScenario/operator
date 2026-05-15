import type { GraphBuilder } from './builder';
import type { Bus } from './bus';
import type { BaseCollection, Collection } from './collection';
import type { PolySignal, StereoOutOptions } from './types';
import { GAIN_CURVE_EXP } from './types';
import { $c } from './collection';
import { captureSourceLocation } from '../captureSourceLocation';

/**
 * ModuleOutput represents an output port that can be connected or transformed.
 */
export class ModuleOutput {
    readonly builder: GraphBuilder;
    readonly moduleId: string;
    readonly portName: string;
    readonly channel: number = 0;

    constructor(
        builder: GraphBuilder,
        moduleId: string,
        portName: string,
        channel: number = 0,
    ) {
        this.builder = builder;
        this.moduleId = moduleId;
        this.portName = portName;
        this.channel = channel;
    }

    /**
     * Scale this output by a linear factor (5 = unity, 2.5 = half, 10 = 2x).
     *
     * For perceptual (audio-taper) volume control, use {@link gain} instead.
     */
    amplitude(factor: PolySignal): Collection {
        const factory = this.builder.getFactory('$scaleAndShift');
        return factory(this, factor) as Collection;
    }

    /** Alias for {@link amplitude} */
    amp(factor: PolySignal): Collection {
        return this.amplitude(factor);
    }

    /** Shift this output by an offset */
    shift(offset: PolySignal): Collection {
        const factory = this.builder.getFactory('$scaleAndShift');
        return factory(this, undefined, offset) as Collection;
    }

    /**
     * Scale this output by a factor with a perceptual (audio taper) curve
     * (5 = unity, 0 = silence). Chains $curve → $scaleAndShift with exponent 3.
     */
    gain(level: PolySignal): Collection {
        const curveFactory = this.builder.getFactory('$curve');
        const scaleFactory = this.builder.getFactory('$scaleAndShift');
        const curvedLevel = curveFactory(level, GAIN_CURVE_EXP);
        return scaleFactory(this, curvedLevel) as Collection;
    }

    /** Apply a power curve to this output. Creates a $curve module internally. */
    exp(factor: PolySignal = GAIN_CURVE_EXP): Collection {
        const factory = this.builder.getFactory('$curve');
        return factory(this, factor) as Collection;
    }

    scope(config?: {
        msPerFrame?: number;
        triggerThreshold?: number;
        triggerWaitToRender?: boolean;
        range?: [number, number];
    }): this {
        const loc = captureSourceLocation();
        this.builder.addScope(this, config, loc);
        return this;
    }

    out(options: StereoOutOptions = {}): this {
        this.builder.addOut(this, { baseChannel: 0, ...options });
        return this;
    }

    outMono(channel: number = 0, gain?: PolySignal): this {
        this.builder.addOutMono(this, { channel, gain });
        return this;
    }

    send(bus: Bus, gain?: PolySignal): this {
        bus.addSend(this, gain);
        return this;
    }

    pipe<U>(pipelineFunc: (self: this) => U): U;
    pipe<U extends ModuleOutput | Iterable<ModuleOutput>, E>(
        pipelineFunc: (self: this, item: E) => U,
        array: E[],
    ): Collection;
    pipe<U>(
        pipelineFunc: (self: this, ...args: unknown[]) => U,
        ...arrays: unknown[][]
    ): U | Collection {
        if (arrays.length === 0) {
            return pipelineFunc(this);
        }
        return $c(
            ...arrays[0].map(
                (item) =>
                    pipelineFunc(this, item) as
                        | ModuleOutput
                        | Iterable<ModuleOutput>,
            ),
        );
    }

    pipeMix(
        pipelineFunc: (
            self: this,
        ) => ModuleOutput | BaseCollection<ModuleOutput>,
        options?: Record<string, unknown>,
    ): Collection {
        const mixFactory = this.builder.getFactory('$mix');
        const result = pipelineFunc(this);
        return mixFactory([this, result], options) as Collection;
    }

    range(
        outMin: PolySignal,
        outMax: PolySignal,
        inMin: PolySignal,
        inMax: PolySignal,
    ): Collection {
        const factory = this.builder.getFactory('$remap');
        return factory(this, outMin, outMax, inMin, inMax) as Collection;
    }

    toString(): string {
        return `module(${this.moduleId}:${this.portName}:${this.channel})`;
    }
}
