import { describe, expect, it } from 'vitest';
import type { VideoGraph } from '../../../../shared/video/videoGraph';
import { compileVideoGraph } from '../wgslCompiler';
import { rampInputs, stripes } from './wgslFixtures';

describe('compileVideoGraph feedback', () => {
    const constant = (value: number) => ({ kind: 'const', value }) as const;
    const loop: VideoGraph = {
        hasOutput: true,
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
                { type: 'field', value: { kind: 'const', value: 1.25 } },
            ]),
        );
        expect(shader.wgsl).toContain('vec3f((u.time * 0.2))');
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
