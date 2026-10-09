import { describe, expect, it } from 'vitest';
import type { VideoGraph } from '../../../../shared/video/videoGraph';
import { compileVideoGraph } from '../wgslCompiler';

const stripes: VideoGraph = {
    nodes: [
        { id: 'x', kind: 'ramp', inputs: {} },
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
                g: { kind: 'const', value: 0.5 },
                b: { kind: 'const', value: 1 },
            },
        },
        {
            id: 'out',
            kind: 'out',
            inputs: { input: { kind: 'node', id: 'rgb' } },
        },
    ],
    output: 'out',
    uniforms: [{ slot: 0, moduleId: 'knob', value: 3 }],
};

const slots = (n: number) =>
    Array.from({ length: n }, (_, slot) => ({
        slot,
        moduleId: `knob${slot}`,
        value: 0,
    }));

describe('compileVideoGraph', () => {
    it('emits one fused fragment shader for the whole graph', () => {
        expect(compileVideoGraph(stripes).wgsl).toMatchInlineSnapshot(`
          "struct Uniforms {
              time: f32,
              resolution: vec2f,
              slots: array<vec4f, 1>,
          }

          @group(0) @binding(0) var<uniform> u: Uniforms;

          @vertex
          fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
              let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
              return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
          }

          @fragment
          fn fs(@builtin(position) frag: vec4f) -> @location(0) vec4f {
              let uv = vec2f(frag.x / u.resolution.x, 1.0 - frag.y / u.resolution.y);
              let v0: f32 = uv.x;
              let v1: f32 = abs(2.0 * fract(v0 * u.slots[0][0] + u.time) - 1.0);
              let v2: vec3f = clamp(vec3f(v1, 0.5, 1.0), vec3f(0.0), vec3f(1.0));
              let v3: vec3f = clamp(v2, vec3f(0.0), vec3f(1.0));
              return vec4f(v3, 1.0);
          }
          "
        `);
    });

    it('sizes the uniform buffer to header plus whole vec4 slots', () => {
        expect(compileVideoGraph(stripes).uniformFloatCount).toBe(8);
        expect(
            compileVideoGraph({ ...stripes, uniforms: slots(5) })
                .uniformFloatCount,
        ).toBe(12);
    });

    it('addresses uniform slots by vec4 row and component', () => {
        const graph: VideoGraph = {
            ...stripes,
            nodes: stripes.nodes.map((n) =>
                n.id === 'wave'
                    ? {
                          ...n,
                          inputs: {
                              ...n.inputs,
                              freq: { kind: 'uniform', slot: 6 },
                          },
                      }
                    : n,
            ),
            uniforms: slots(7),
        };
        expect(compileVideoGraph(graph).wgsl).toContain('u.slots[1][2]');
    });

    it('formats integer constants as WGSL floats', () => {
        expect(compileVideoGraph(stripes).wgsl).toContain(
            'vec3f(v1, 0.5, 1.0)',
        );
    });

    it.each([
        [
            'unknown module',
            { nodes: [{ id: 'a', kind: 'nope', inputs: {} }], output: 'a' },
            /unknown video module/,
        ],
        [
            'missing input',
            { nodes: [{ id: 'a', kind: 'osc', inputs: {} }], output: 'a' },
            /missing input "input"/,
        ],
        [
            'forward reference',
            {
                nodes: [
                    {
                        id: 'o',
                        kind: 'out',
                        inputs: { input: { kind: 'node', id: 'later' } },
                    },
                    { id: 'later', kind: 'ramp', inputs: {} },
                ],
                output: 'o',
            },
            /unknown or forward node "later"/,
        ],
        [
            'type mismatch',
            {
                nodes: [
                    { id: 'x', kind: 'ramp', inputs: {} },
                    {
                        id: 'o',
                        kind: 'out',
                        inputs: { input: { kind: 'node', id: 'x' } },
                    },
                ],
                output: 'o',
            },
            /is a field, expected a color/,
        ],
        [
            'bad param',
            {
                nodes: [
                    {
                        id: 'x',
                        kind: 'ramp',
                        inputs: {},
                        params: { axis: 'z' },
                    },
                ],
                output: 'x',
            },
            /param "axis" must be one of/,
        ],
        [
            'field output',
            { nodes: [{ id: 'x', kind: 'ramp', inputs: {} }], output: 'x' },
            /must be a color node/,
        ],
        [
            'slot out of range',
            {
                nodes: [
                    { id: 'x', kind: 'ramp', inputs: {} },
                    {
                        id: 'w',
                        kind: 'osc',
                        inputs: {
                            input: { kind: 'node', id: 'x' },
                            freq: { kind: 'uniform', slot: 3 },
                            phase: { kind: 'const', value: 0 },
                        },
                    },
                ],
                output: 'w',
            },
            /slot 3 is out of range/,
        ],
        [
            'non-finite constant',
            {
                nodes: [
                    { id: 'x', kind: 'ramp', inputs: {} },
                    {
                        id: 'w',
                        kind: 'osc',
                        inputs: {
                            input: { kind: 'node', id: 'x' },
                            freq: { kind: 'const', value: Number.NaN },
                            phase: { kind: 'const', value: 0 },
                        },
                    },
                ],
                output: 'w',
            },
            /must be finite/,
        ],
    ] as const)('rejects %s', (_name, partial, message) => {
        const graph = {
            uniforms: [],
            ...partial,
        } as unknown as VideoGraph;
        expect(() => compileVideoGraph(graph)).toThrow(message);
    });
});
