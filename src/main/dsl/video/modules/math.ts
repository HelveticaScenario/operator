import type { VideoModuleDef } from './types';

const unit = (x: string) => `clamp(${x}, 0.0, 1.0)`;
const unit3 = (x: string) => `clamp(${x}, vec3f(0.0), vec3f(1.0))`;

/** Sum, clipped to 0..1. */
export const add: VideoModuleDef = {
    inputs: { a: 'field', b: 'field' },
    output: 'field',
    params: {},
    emit: ({ a, b }) => unit(`${a} + ${b}`),
};

export const addColor: VideoModuleDef = {
    inputs: { a: 'color', b: 'color' },
    output: 'color',
    params: {},
    emit: ({ a, b }) => unit3(`${a} + ${b}`),
};

/** Product; multiplying by a 0..1 field darkens. */
export const mult: VideoModuleDef = {
    inputs: { a: 'field', b: 'field' },
    output: 'field',
    params: {},
    emit: ({ a, b }) => `${a} * ${b}`,
};

export const multColor: VideoModuleDef = {
    inputs: { a: 'color', b: 'color' },
    output: 'color',
    params: {},
    emit: ({ a, b }) => `${a} * ${b}`,
};

/** Absolute difference. */
export const diff: VideoModuleDef = {
    inputs: { a: 'field', b: 'field' },
    output: 'field',
    params: {},
    emit: ({ a, b }) => `abs(${a} - ${b})`,
};

export const diffColor: VideoModuleDef = {
    inputs: { a: 'color', b: 'color' },
    output: 'color',
    params: {},
    emit: ({ a, b }) => `abs(${a} - ${b})`,
};

/** Complement: 1 - input. */
export const invert: VideoModuleDef = {
    inputs: { input: 'field' },
    output: 'field',
    params: {},
    emit: ({ input }) => `1.0 - ${unit(input)}`,
};

export const invertColor: VideoModuleDef = {
    inputs: { input: 'color' },
    output: 'color',
    params: {},
    emit: ({ input }) => `vec3f(1.0) - ${unit3(input)}`,
};

/** Crossfade from `a` (amount 0) to `b` (amount 1). */
export const mix: VideoModuleDef = {
    inputs: { a: 'field', b: 'field', amount: 'field' },
    output: 'field',
    params: {},
    emit: ({ a, b, amount }) => `mix(${a}, ${b}, ${unit(amount)})`,
};

export const mixColor: VideoModuleDef = {
    inputs: { a: 'color', b: 'color', amount: 'field' },
    output: 'color',
    params: {},
    emit: ({ a, b, amount }) => `mix(${a}, ${b}, ${unit(amount)})`,
};

/** The larger of two values; with 0..1 fields this is a union of shapes. */
export const max: VideoModuleDef = {
    inputs: { a: 'field', b: 'field' },
    output: 'field',
    params: {},
    emit: ({ a, b }) => `max(${a}, ${b})`,
};

export const maxColor: VideoModuleDef = {
    inputs: { a: 'color', b: 'color' },
    output: 'color',
    params: {},
    emit: ({ a, b }) => `max(${a}, ${b})`,
};

/** The smaller of two values; with 0..1 fields this is an intersection of shapes. */
export const min: VideoModuleDef = {
    inputs: { a: 'field', b: 'field' },
    output: 'field',
    params: {},
    emit: ({ a, b }) => `min(${a}, ${b})`,
};

export const minColor: VideoModuleDef = {
    inputs: { a: 'color', b: 'color' },
    output: 'color',
    params: {},
    emit: ({ a, b }) => `min(${a}, ${b})`,
};

/** Maps 0..5 volts onto `min`..`max` volts: what `.range` does to a field. */
export const range: VideoModuleDef = {
    inputs: { input: 'field', min: 'field', max: 'field' },
    natural: ['min', 'max'],
    output: 'field',
    params: {},
    emit: (args) =>
        `((${args.min} + ${args.input} * (${args.max} - ${args.min})) * 0.2)`,
};
