import { describe, expect, it } from 'vitest';
import type { VideoGraph } from '../../../../shared/video/videoGraph';
import { compileVideoGraph } from '../wgslCompiler';
import { rampInputs, stripes, slots } from './wgslFixtures';

describe('compileVideoGraph', () => {
    it('emits one fused fragment shader for the whole graph', () => {
        expect(compileVideoGraph(stripes).wgsl).toMatchInlineSnapshot(`
          "struct Uniforms {
              time: f32,
              resolution: vec2f,
              slots: array<vec4f, 1>,
          }

          @group(0) @binding(0) var<uniform> u: Uniforms;

          fn video_transform(uv: vec2f, zoom: f32, rotate: f32, shift: vec2f) -> vec2f {
              let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
              let z = select(-1.0, 1.0, zoom >= 0.0) * max(abs(zoom), 0.00001);
              let p = (uv - vec2f(0.5) - shift) * aspect / z;
              let a = rotate * 6.28318530718;
              let c = cos(a);
              let s = sin(a);
              return vec2f(c * p.x - s * p.y, s * p.x + c * p.y) / aspect + vec2f(0.5);
          }

          fn ramp_radius(q: vec2f) -> f32 {
              return length((q - vec2f(0.5)) * vec2f(u.resolution.x / u.resolution.y, 1.0));
          }

          fn ramp_angle(q: vec2f) -> f32 {
              let p = (q - vec2f(0.5)) * vec2f(u.resolution.x / u.resolution.y, 1.0);
              return atan2(p.y, p.x) / 6.28318530718 + 0.5;
          }

          fn video_luma(c: vec4f) -> f32 {
              return dot(c.rgb, vec3f(0.299, 0.587, 0.114));
          }

          fn video_display(c: vec4f) -> vec4f {
              let clipped = clamp(c, vec4f(0.0), vec4f(1.0));
              return vec4f(clipped.rgb * clipped.a, 1.0);
          }

          @vertex
          fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
              let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
              return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
          }

          @fragment
          fn fs(@builtin(position) frag: vec4f) -> @location(0) vec4f {
              let uv = vec2f(frag.x / u.resolution.x, 1.0 - frag.y / u.resolution.y);
              let v0: vec4f = vec4f(vec3f(uv.x), 1.0);
              let v1: vec4f = vec4f(vec3f(abs(2.0 * fract(video_luma(v0) * u.slots[0][0] + u.time) - 1.0)), 1.0);
              let v2: vec4f = clamp(vec4f(video_luma(v1), 0.5, 1.0, 1.0), vec4f(0.0), vec4f(1.0));
              let v3: vec4f = v2;
              return video_display(v3);
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
            'vec4f(video_luma(v1), 0.5, 1.0, 1.0)',
        );
    });

    it('reads a node as its luma where a number is wanted, and whole where a color is', () => {
        const { wgsl } = compileVideoGraph(stripes);
        expect(wgsl).toContain('fn video_luma(c: vec4f) -> f32');
        expect(wgsl).toContain(
            'let v1: vec4f = vec4f(vec3f(abs(2.0 * fract(video_luma(v0)',
        );
        expect(wgsl).toContain('let v3: vec4f = v2;');
    });

    it('gives a number to a color input as an opaque gray', () => {
        const { wgsl } = compileVideoGraph({
            ...stripes,
            nodes: [
                {
                    id: 'o',
                    kind: 'out',
                    inputs: { input: { kind: 'const', value: 0.25 } },
                },
            ],
            output: 'o',
        });
        expect(wgsl).toContain('let v0: vec4f = vec4f(vec3f(0.25), 1.0);');
    });

    it('shows the output laid over black by its alpha', () => {
        const { wgsl } = compileVideoGraph(stripes);
        expect(wgsl).toContain('return vec4f(clipped.rgb * clipped.a, 1.0);');
        expect(wgsl).toContain('return video_display(v3);');
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
                    { id: 'later', kind: 'ramp', inputs: rampInputs },
                ],
                output: 'o',
            },
            /unknown or forward node "later"/,
        ],
        [
            'bad param',
            {
                nodes: [
                    {
                        id: 'x',
                        kind: 'ramp',
                        inputs: rampInputs,
                        params: { axis: 'z' },
                    },
                ],
                output: 'x',
            },
            /param "axis" must be one of/,
        ],
        [
            'output that is not a node',
            {
                nodes: [{ id: 'x', kind: 'ramp', inputs: rampInputs }],
                output: 'y',
            },
            /video output "y" is not a node/,
        ],
        [
            'slot out of range',
            {
                nodes: [
                    { id: 'x', kind: 'ramp', inputs: rampInputs },
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
                    { id: 'x', kind: 'ramp', inputs: rampInputs },
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
        [
            'constant beyond the f32 range',
            {
                nodes: [
                    { id: 'x', kind: 'ramp', inputs: rampInputs },
                    {
                        id: 'w',
                        kind: 'osc',
                        inputs: {
                            input: { kind: 'node', id: 'x' },
                            freq: { kind: 'const', value: 1e39 },
                            phase: { kind: 'const', value: 0 },
                        },
                    },
                ],
                output: 'w',
            },
            /within the f32 range/,
        ],
    ] as const)('rejects %s', (_name, partial, message) => {
        const graph = {
            histories: [],
            sources: [],
            previews: [],
            uniforms: [],
            ...partial,
        } as unknown as VideoGraph;
        expect(() => compileVideoGraph(graph)).toThrow(message);
    });
});
