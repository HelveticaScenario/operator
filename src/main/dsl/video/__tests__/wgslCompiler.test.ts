import { describe, expect, it } from 'vitest';
import type { VideoGraph } from '../../../../shared/video/videoGraph';
import { compileVideoGraph } from '../wgslCompiler';

/** Identity transform: the ramp compiles to the bare frame coordinate. */
const rampInputs = {
    zoom: { kind: 'const', value: 1 },
    rotate: { kind: 'const', value: 0 },
    shiftX: { kind: 'const', value: 0 },
    shiftY: { kind: 'const', value: 0 },
} as const;

const stripes: VideoGraph = {
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
    histories: [],
    output: 'out',
    sources: [],
    previews: [],
    uniforms: [{ kind: 'control', slot: 0, moduleId: 'knob', value: 3 }],
};

const slots = (n: number) =>
    Array.from({ length: n }, (_, slot) => ({
        kind: 'control' as const,
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

          fn video_transform(uv: vec2f, zoom: f32, rotate: f32, shift: vec2f) -> vec2f {
              let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
              let p = (uv - vec2f(0.5) - shift) * aspect / max(zoom, 0.00001);
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
                    { id: 'later', kind: 'ramp', inputs: rampInputs },
                ],
                output: 'o',
            },
            /unknown or forward node "later"/,
        ],
        [
            'type mismatch',
            {
                nodes: [
                    { id: 'x', kind: 'ramp', inputs: rampInputs },
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
                        inputs: rampInputs,
                        params: { axis: 'z' },
                    },
                ],
                output: 'x',
            },
            /param "axis" must be one of/,
        ],
        [
            'field output',
            {
                nodes: [{ id: 'x', kind: 'ramp', inputs: rampInputs }],
                output: 'x',
            },
            /must be a color node/,
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

describe('compileVideoGraph feedback', () => {
    const constant = (value: number) => ({ kind: 'const', value }) as const;
    const loop: VideoGraph = {
        nodes: [
            {
                id: 'prev',
                kind: 'feedbackRead',
                inputs: {
                    zoom: constant(1.01),
                    rotate: constant(0),
                    shiftX: constant(0),
                    shiftY: constant(0),
                },
                params: { edge: 'mirror' },
                buffer: 0,
            },
            {
                id: 'frame',
                kind: 'mixColor',
                inputs: {
                    a: { kind: 'node', id: 'prev' },
                    b: { kind: 'node', id: 'prev' },
                    amount: constant(0.5),
                },
            },
            {
                id: 'store',
                kind: 'feedbackWrite',
                inputs: { input: { kind: 'node', id: 'frame' } },
                buffer: 0,
            },
            {
                id: 'out',
                kind: 'out',
                inputs: { input: { kind: 'node', id: 'frame' } },
            },
        ],
        histories: [],
        output: 'out',
        previews: [],
        sources: [],
        uniforms: [],
    };

    it('reads and writes buffers through extra bindings and render targets', () => {
        const shader = compileVideoGraph(loop);
        expect(shader.feedbackBufferCount).toBe(1);
        expect(shader.wgsl).toContain('@group(0) @binding(1) var fb_sampler');
        expect(shader.wgsl).toContain(
            '@group(0) @binding(2) var fb_0: texture_2d<f32>;',
        );
        expect(shader.wgsl).toContain('@location(1) fb0: vec4f');
        expect(shader.wgsl).toContain(
            'feedback_mirror(video_transform(uv, 1.01',
        );
        expect(shader.wgsl).toMatch(
            /return FragOut\(vec4f\(v3, 1.0\), vec4f\(v2, 1.0\)\);/,
        );
    });

    it('declares no buffers for a graph without feedback', () => {
        const shader = compileVideoGraph(stripes);
        expect(shader.feedbackBufferCount).toBe(0);
        expect(shader.wgsl).not.toContain('fb_sampler');
    });

    it('rejects a buffer that is read but never written', () => {
        const graph = {
            ...loop,
            nodes: loop.nodes.filter((n) => n.id !== 'store'),
        };
        expect(() => compileVideoGraph(graph)).toThrow(
            /feedback buffer 0 is read but never written/,
        );
    });

    it('rejects a buffer written twice', () => {
        const store = loop.nodes.find((n) => n.id === 'store')!;
        const graph = {
            ...loop,
            nodes: [...loop.nodes, { ...store, id: 'store2' }],
        };
        expect(() => compileVideoGraph(graph)).toThrow(/written twice/);
    });

    it('rejects an out-of-range buffer and a buffer on a plain module', () => {
        const bad = (buffer: number) => ({
            ...loop,
            nodes: loop.nodes.map((n) =>
                n.id === 'prev' ? { ...n, buffer } : n,
            ),
        });
        expect(() => compileVideoGraph(bad(7))).toThrow(/integer from 0 to 6/);
        expect(() =>
            compileVideoGraph({
                ...stripes,
                nodes: stripes.nodes.map((n) =>
                    n.id === 'x' ? { ...n, buffer: 0 } : n,
                ),
            }),
        ).toThrow(/module has no feedback buffer/);
    });
});

describe('compileVideoGraph previews', () => {
    const withPreviews = (previews: VideoGraph['previews']): VideoGraph => ({
        ...stripes,
        previews,
    });

    it('adds one fragment entry point per preview', () => {
        const shader = compileVideoGraph(
            withPreviews([
                { type: 'field', value: { kind: 'node', id: 'wave' } },
                { type: 'color', value: { kind: 'node', id: 'rgb' } },
            ]),
        );
        expect(shader.previewCount).toBe(2);
        expect(shader.wgsl).toContain('fn preview_0(');
        expect(shader.wgsl).toContain('fn preview_1(');
        expect(shader.wgsl).toContain('return vec4f(vec3f(v1), 1.0);');
        expect(shader.wgsl).toContain('return vec4f(v2, 1.0);');
    });

    it('evaluates only the nodes a preview depends on', () => {
        const shader = compileVideoGraph(
            withPreviews([{ type: 'field', value: { kind: 'node', id: 'x' } }]),
        );
        const entry = shader.wgsl.slice(shader.wgsl.indexOf('fn preview_0('));
        expect(entry).toContain('let v0: f32 = uv.x;');
        expect(entry).not.toContain('let v1');
        expect(entry).not.toContain('let v2');
    });

    it('previews a constant or a time input directly', () => {
        const shader = compileVideoGraph(
            withPreviews([
                { type: 'field', value: { kind: 'time' } },
                { type: 'field', value: { kind: 'const', value: 0.25 } },
            ]),
        );
        expect(shader.wgsl).toContain('vec3f(u.time)');
        expect(shader.wgsl).toContain('vec3f(0.25)');
    });

    it('declares no previews by default', () => {
        const shader = compileVideoGraph(stripes);
        expect(shader.previewCount).toBe(0);
        expect(shader.wgsl).not.toContain('preview_');
    });

    it('rejects a preview whose type does not match its value', () => {
        expect(() =>
            compileVideoGraph(
                withPreviews([
                    { type: 'color', value: { kind: 'node', id: 'x' } },
                ]),
            ),
        ).toThrow(/node "x" is a field, expected a color/);
    });
});

describe('compileVideoGraph audio history', () => {
    const history: VideoGraph = {
        ...stripes,
        histories: [{ samples: 512, tap: 3, trigger: true }],
        nodes: [
            { id: 'x', kind: 'ramp', inputs: rampInputs },
            {
                id: 'wave',
                kind: 'audioHistory',
                history: 0,
                inputs: {
                    position: { kind: 'node', id: 'x' },
                    samples: { kind: 'const', value: 512 },
                },
            },
            {
                id: 'rgb',
                kind: 'colorize',
                inputs: {
                    r: { kind: 'node', id: 'wave' },
                    g: { kind: 'const', value: 0 },
                    b: { kind: 'const', value: 0 },
                },
            },
            {
                id: 'out',
                kind: 'out',
                inputs: { input: { kind: 'node', id: 'rgb' } },
            },
        ],
    };

    it('reads a row of the history texture', () => {
        const shader = compileVideoGraph(history);
        expect(shader.wgsl).toContain(
            '@group(0) @binding(2) var history_tex: texture_2d<f32>;',
        );
        expect(shader.wgsl).toContain('history_sample(0, v0, 512.0)');
        expect(shader.histories).toEqual(history.histories);
    });

    it('binds the history texture after the feedback textures', () => {
        const withFeedback: VideoGraph = {
            ...history,
            nodes: [
                ...history.nodes.slice(0, 3),
                {
                    id: 'prev',
                    kind: 'feedbackRead',
                    buffer: 0,
                    inputs: {
                        zoom: { kind: 'const', value: 1 },
                        rotate: { kind: 'const', value: 0 },
                        shiftX: { kind: 'const', value: 0 },
                        shiftY: { kind: 'const', value: 0 },
                    },
                },
                {
                    id: 'store',
                    kind: 'feedbackWrite',
                    buffer: 0,
                    inputs: { input: { kind: 'node', id: 'prev' } },
                },
                history.nodes[3],
            ],
        };
        expect(compileVideoGraph(withFeedback).wgsl).toContain(
            '@group(0) @binding(3) var history_tex',
        );
    });

    it('declares no history texture for a graph without audio history', () => {
        expect(compileVideoGraph(stripes).wgsl).not.toContain('history_tex');
    });

    it('rejects a row beyond the declared histories', () => {
        const bad = {
            ...history,
            nodes: history.nodes.map((n) =>
                n.id === 'wave' ? { ...n, history: 1 } : n,
            ),
        };
        expect(() => compileVideoGraph(bad)).toThrow(
            /audio history row must be an integer from 0 to 0/,
        );
    });

    it('rejects a history row on a module that does not read one', () => {
        const bad = {
            ...stripes,
            nodes: stripes.nodes.map((n) =>
                n.id === 'x' ? { ...n, history: 0 } : n,
            ),
        };
        expect(() => compileVideoGraph(bad)).toThrow(
            /module has no audio history/,
        );
    });
});

describe('compileVideoGraph warped inputs', () => {
    const warpGraph = (inputType: 'field' | 'color'): VideoGraph => ({
        ...stripes,
        nodes: [
            { id: 'x', kind: 'ramp', inputs: rampInputs },
            {
                id: 'shifted',
                kind: inputType === 'color' ? 'pixelateColor' : 'pixelate',
                inputs: {
                    input: { kind: 'node', id: 'x' },
                    x: { kind: 'const', value: 8 },
                    y: { kind: 'const', value: 8 },
                },
            },
            {
                id: 'rgb',
                kind: 'colorize',
                inputs: {
                    r: { kind: 'node', id: 'shifted' },
                    g: { kind: 'const', value: 0 },
                    b: { kind: 'const', value: 0 },
                },
            },
            {
                id: 'out',
                kind: 'out',
                inputs: { input: { kind: 'node', id: 'rgb' } },
            },
        ],
    });

    it('hands a warped input to the module as a function name', () => {
        const wgsl = compileVideoGraph(warpGraph('field')).wgsl;
        expect(wgsl).toContain('fn f0(uv: vec2f) -> f32 {\n    return uv.x;');
        expect(wgsl).toContain(
            'let v1: f32 = f0(video_pixelate(uv, vec2f(8.0, 8.0)));',
        );
    });

    it('checks a warped input against the type its module declares', () => {
        expect(() => compileVideoGraph(warpGraph('color'))).toThrow(
            /node "x" is a field, expected a color/,
        );
    });

    it('writes no functions for a graph without warps', () => {
        expect(compileVideoGraph(stripes).wgsl).not.toMatch(/fn f\d+\(/);
    });
});
