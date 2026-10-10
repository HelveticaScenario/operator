import type { VideoValueType } from '../../../../shared/video/videoGraph';
import type { VideoModuleDef } from './types';

const unit = (x: string) => `clamp(${x}, 0.0, 1.0)`;
const unit3 = (x: string) => `clamp(${x}, vec3f(0.0), vec3f(1.0))`;

/**
 * The field and color variants of a module whose `operands` are both fields
 * or both colors; `fields` are field inputs in either variant. `emit` receives
 * the variant's type along with the inputs.
 */
function both(
    operands: readonly string[],
    emit: (args: Record<string, string>, type: VideoValueType) => string,
    fields: readonly string[] = [],
): { field: VideoModuleDef; color: VideoModuleDef } {
    const make = (type: VideoValueType): VideoModuleDef => ({
        inputs: {
            ...Object.fromEntries(operands.map((name) => [name, type])),
            ...Object.fromEntries(fields.map((name) => [name, 'field'])),
        },
        output: type,
        emit: (args) => emit(args, type),
    });
    return { field: make('field'), color: make('color') };
}

/** Sum, clipped to 0..1. */
export const add = both(['a', 'b'], ({ a, b }, type) =>
    (type === 'field' ? unit : unit3)(`${a} + ${b}`),
);

/** Product; multiplying by a 0..1 field darkens. */
export const mult = both(['a', 'b'], ({ a, b }) => `${a} * ${b}`);

/** Absolute difference. */
export const diff = both(['a', 'b'], ({ a, b }) => `abs(${a} - ${b})`);

/** Complement: 1 - input. */
export const invert = both(['input'], ({ input }, type) =>
    type === 'field' ? `1.0 - ${unit(input)}` : `vec3f(1.0) - ${unit3(input)}`,
);

/** Crossfade from `a` (amount 0) to `b` (amount 1). */
export const mix = both(
    ['a', 'b'],
    ({ a, b, amount }) => `mix(${a}, ${b}, ${unit(amount)})`,
    ['amount'],
);

/** The larger of two values; with 0..1 fields this is a union of shapes. */
export const max = both(['a', 'b'], ({ a, b }) => `max(${a}, ${b})`);

/** The smaller of two values; with 0..1 fields this is an intersection of shapes. */
export const min = both(['a', 'b'], ({ a, b }) => `min(${a}, ${b})`);

/** Maps 0..5 volts onto `min`..`max` volts: what `.range` does to a field. */
export const range: VideoModuleDef = {
    inputs: { input: 'field', min: 'field', max: 'field' },
    natural: ['min', 'max'],
    output: 'field',
    emit: (args) =>
        `((${args.min} + ${args.input} * (${args.max} - ${args.min})) * 0.2)`,
};

/**
 * Threshold: 0 below `threshold`, 1 above, with a linear ramp `softness` wide
 * centered on it (a hard edge when softness is 0).
 */
export const comparator: VideoModuleDef = {
    inputs: { input: 'field', threshold: 'field', softness: 'field' },
    output: 'field',
    emit: ({ input, threshold, softness }) =>
        `clamp((${input} - ${threshold}) / max(${softness}, 0.00001) + 0.5, 0.0, 1.0)`,
};

/** Shows `fg` where `mask` is 1 and `bg` where it is 0. */
export const key: VideoModuleDef = {
    inputs: { fg: 'color', bg: 'color', mask: 'field' },
    output: 'color',
    emit: ({ fg, bg, mask }) => `mix(${bg}, ${fg}, ${unit(mask)})`,
};

/** Combines three fields into a color, clipping each channel to 0..1. */
export const colorize: VideoModuleDef = {
    inputs: { r: 'field', g: 'field', b: 'field' },
    output: 'color',
    emit: ({ r, g, b }) => unit3(`vec3f(${r}, ${g}, ${b})`),
};

/** Final stage before the display; clips to the displayable range. */
export const out: VideoModuleDef = {
    inputs: { input: 'color' },
    output: 'color',
    emit: ({ input }) => unit3(input),
};
