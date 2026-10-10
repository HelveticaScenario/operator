import type { VideoGraph } from '../../../../shared/video/videoGraph';

/** Identity transform: the ramp compiles to the bare frame coordinate. */
export const rampInputs = {
    zoom: { kind: 'const', value: 1 },
    rotate: { kind: 'const', value: 0 },
    shiftX: { kind: 'const', value: 0 },
    shiftY: { kind: 'const', value: 0 },
} as const;

export const stripes: VideoGraph = {
    hasOutput: true,
    nodes: [
        { id: 'x', kind: 'ramp', inputs: rampInputs },
        {
            id: 'wave',
            kind: 'osc',
            inputs: {
                input: { kind: 'node', id: 'x' },
                freq: { kind: 'uniform', slot: 0 },
                phase: { kind: 'time' },
            },
            params: { shape: 'triangle' },
        },
        {
            id: 'rgb',
            kind: 'colorize',
            inputs: {
                r: { kind: 'node', id: 'wave' },
                g: { kind: 'const', value: 2.5 },
                b: { kind: 'const', value: 5 },
            },
        },
        {
            id: 'out',
            kind: 'out',
            inputs: { input: { kind: 'node', id: 'rgb' } },
        },
    ],
    histories: [],
    output: 'out',
    sources: [],
    previews: [],
    uniforms: [{ kind: 'control', slot: 0, moduleId: 'knob', value: 3 }],
};

export const slots = (n: number) =>
    Array.from({ length: n }, (_, slot) => ({
        kind: 'control' as const,
        slot,
        moduleId: `knob${slot}`,
        value: 0,
    }));
