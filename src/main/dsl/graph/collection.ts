import type { Bus } from './bus';
import type { ModuleOutput } from './moduleOutput';
import type { PolySignal, StereoOutOptions } from './types';
import { GAIN_CURVE_EXP } from './types';
import { captureSourceLocation } from '../captureSourceLocation';

/**
 * BaseCollection provides iterable, indexable container for ModuleOutput arrays
 * with chainable DSP methods (amplitude, shift, scope, out).
 */
export class BaseCollection<T extends ModuleOutput> implements Iterable<T> {
    [index: number]: T;
    readonly items: T[] = [];

    constructor(...args: T[]) {
        this.items.push(...args);
        for (const [i, arg] of args.entries()) {
            this[i] = arg;
        }
    }

    get length(): number {
        return this.items.length;
    }

    [Symbol.iterator](): Iterator<T> {
        return this.items.values();
    }

    /** Scale all outputs by a linear factor (5 = unity, 2.5 = half, 10 = 2x). */
    amplitude(factor: PolySignal): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
        const factory = this.items[0].builder.getFactory('$scaleAndShift');
        if (!factory) {
            throw new Error('Factory for util.scaleAndShift not registered');
        }
        return factory(this.items, factor) as Collection;
    }

    /** Alias for {@link amplitude} */
    amp(factor: PolySignal): Collection {
        return this.amplitude(factor);
    }

    /** Shift all outputs by an offset */
    shift(offset: PolySignal): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
        const factory = this.items[0].builder.getFactory('$scaleAndShift');
        if (!factory) {
            throw new Error('Factory for util.scaleAndShift not registered');
        }
        return factory(this.items, undefined, offset) as Collection;
    }

    /**
     * Scale all outputs by a factor with a perceptual (audio taper) curve.
     * Chains $curve → $scaleAndShift with exponent 3.
     */
    gain(level: PolySignal): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
        const curveFactory = this.items[0].builder.getFactory('$curve');
        const scaleFactory = this.items[0].builder.getFactory('$scaleAndShift');
        if (!curveFactory || !scaleFactory) {
            throw new Error(
                'Factory for $curve or $scaleAndShift not registered',
            );
        }
        const curvedLevel = curveFactory(level, GAIN_CURVE_EXP);
        return scaleFactory(this.items, curvedLevel) as Collection;
    }

    /** Apply a power curve to all outputs. Creates a $curve module internally. */
    exp(factor: PolySignal = GAIN_CURVE_EXP): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
        const factory = this.items[0].builder.getFactory('$curve');
        if (!factory) {
            throw new Error('Factory for $curve not registered');
        }
        return factory(this.items, factor) as Collection;
    }

    scope(config?: {
        msPerFrame?: number;
        triggerThreshold?: number;
        triggerWaitToRender?: boolean;
        range?: [number, number];
    }): this {
        if (this.items.length > 0) {
            const loc = captureSourceLocation();
            this.items[0].builder.addScope(this.items, config, loc);
        }
        return this;
    }

    out(options: StereoOutOptions = {}): this {
        if (this.items.length > 0) {
            this.items[0].builder.addOut([...this.items], {
                baseChannel: 0,
                ...options,
            });
        }
        return this;
    }

    outMono(channel: number = 0, gain?: PolySignal): this {
        if (this.items.length > 0) {
            this.items[0].builder.addOutMono([...this.items], {
                channel,
                gain,
            });
        }
        return this;
    }

    send(bus: Bus, gain?: PolySignal): this {
        bus.addSend([...this], gain);
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
        mix: PolySignal = 2.5,
    ): Collection {
        const clampFactory = this.items[0].builder.getFactory('$clamp');
        if (!clampFactory) {
            throw new Error('Factory for $clamp not registered');
        }
        const remapFactory = this.items[0].builder.getFactory('$remap');
        if (!remapFactory) {
            throw new Error('Factory for $remap not registered');
        }
        const mixFactory = this.items[0].builder.getFactory('$mix');
        if (!mixFactory) {
            throw new Error('Factory for $mix not registered');
        }
        const result = pipelineFunc(this);
        // Remap mix from 0-5 to 5-0 for crossfade between original and transformed signals
        return mixFactory([
            this.amplitude(
                clampFactory(remapFactory(mix, 5, 0, 0, 5), {
                    max: 5,
                    min: 0,
                }) as PolySignal,
            ),
            result.amplitude(
                clampFactory(mix, { max: 5, min: 0 }) as PolySignal,
            ),
        ]) as Collection;
    }

    toString(): string {
        return `[${this.items.map((item) => item.toString()).join(',')}]`;
    }
}

/**
 * Collection of ModuleOutput instances.
 * Use .range(outMin, outMax, inMin, inMax) to remap with explicit input range.
 */
export class Collection extends BaseCollection<ModuleOutput> {
    /** Remap outputs from explicit input range to output range */
    range(
        outMin: PolySignal,
        outMax: PolySignal,
        inMin: PolySignal,
        inMax: PolySignal,
    ): Collection {
        if (this.items.length === 0) {
            return new Collection();
        }
        const factory = this.items[0].builder.getFactory('$remap');
        if (!factory) {
            throw new Error('Factory for util.remap not registered');
        }
        return factory(this.items, outMin, outMax, inMin, inMax) as Collection;
    }
}

/** Create a Collection from ModuleOutput instances */
// Lazy import of ModuleOutput keeps the value-level cycle from triggering at module load.
import { ModuleOutput as ModuleOutputClass } from './moduleOutput';

export const $c = (
    ...args: (ModuleOutput | Iterable<ModuleOutput>)[]
): Collection =>
    new Collection(
        ...args.flatMap((arg) =>
            arg instanceof ModuleOutputClass ? [arg] : [...arg],
        ),
    );
