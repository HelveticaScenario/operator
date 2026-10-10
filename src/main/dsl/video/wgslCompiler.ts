import {
    MAX_FEEDBACK_BUFFERS,
    type CompiledVideoShader,
    type VideoGraph,
    type VideoValue,
    type VideoValueType,
} from '../../../shared/video/videoGraph';
import {
    UNIFORM_SLOTS_OFFSET,
    bindingSlots,
} from '../../../shared/video/uniformLayout';
import { VIDEO_MODULES } from './modules';
import type { VideoModuleDef } from './modules/types';

/** A constant in volts as a fraction of 5 volts, without floating-point noise. */
function fractionOfFullScale(volts: number): number {
    return Number((volts / 5).toPrecision(9));
}

function wgslFloat(value: number): string {
    if (!Number.isFinite(value)) {
        throw new Error(`video constant must be finite, got ${value}`);
    }
    const text = String(value);
    return /^-?\d+$/.test(text) ? `${text}.0` : text;
}

/**
 * Compiles a video graph to one WGSL module with a fullscreen-triangle vertex
 * stage and a fragment stage that evaluates every node per pixel.
 */
export function compileVideoGraph(graph: VideoGraph): CompiledVideoShader {
    const types = new Map<string, VideoValueType>();
    const names = new Map<string, string>();
    const lines: string[] = [];
    const helpers = new Set<string>();
    /** Each node's WGSL statement, with the nodes it reads, in graph order. */
    const statements: { id: string; text: string; reads: string[] }[] = [];
    /** Local variable holding what each feedback buffer stores this frame. */
    const bufferWrites = new Map<number, string>();
    let bufferCount = 0;

    /**
     * Each node as a function of the coordinate, `fn f<index>(uv) -> T`, for
     * modules that re-evaluate an input at other coordinates. Written on
     * demand; the statement per node above is the same node at the pixel's own
     * coordinate, computed once and shared.
     */
    interface NodeRecord {
        index: number;
        node: VideoGraph['nodes'][number];
        def: VideoModuleDef;
        params: Record<string, string>;
        indices: { buffer: number; history: number; source: number };
    }
    const records = new Map<string, NodeRecord>();
    const functions: string[] = [];
    const functionsDone = new Set<string>();
    const constantFunctions = new Map<string, string>();
    const wgslType = (type: VideoValueType) =>
        type === 'field' ? 'f32' : 'vec3f';

    /**
     * The WGSL for an input. Modules compute with 5 volts as 1, so a constant,
     * an audio signal or the clock is divided by 5 on its way in, and a
     * field's value is the volts of a `natural` input times 5 on its way out.
     */
    const resolve = (
        value: VideoValue,
        expected: VideoValueType,
        natural = false,
    ): string => {
        switch (value.kind) {
            case 'const':
            case 'uniform':
            case 'time':
                if (expected !== 'field') {
                    throw new Error(
                        `expected a ${expected}, got a ${value.kind}`,
                    );
                }
                if (value.kind === 'const') {
                    return wgslFloat(
                        natural
                            ? value.value
                            : fractionOfFullScale(value.value),
                    );
                }
                if (value.kind === 'time') {
                    return natural ? 'u.time' : '(u.time * 0.2)';
                }
                if (
                    !Number.isInteger(value.slot) ||
                    value.slot < 0 ||
                    value.slot >= graph.uniforms.length
                ) {
                    throw new Error(
                        `uniform slot ${value.slot} is out of range`,
                    );
                }
                return natural
                    ? `u.slots[${value.slot >> 2}][${value.slot & 3}]`
                    : `(u.slots[${value.slot >> 2}][${value.slot & 3}] * 0.2)`;
            case 'node': {
                const actual = types.get(value.id);
                if (actual === undefined) {
                    throw new Error(`unknown or forward node "${value.id}"`);
                }
                if (actual !== expected) {
                    throw new Error(
                        `node "${value.id}" is a ${actual}, expected a ${expected}`,
                    );
                }
                return natural
                    ? `(${names.get(value.id)!} * 5.0)`
                    : names.get(value.id)!;
            }
        }
    };

    const checkNodeType = (id: string, expected: VideoValueType) => {
        const actual = types.get(id);
        if (actual === undefined) {
            throw new Error(`unknown or forward node "${id}"`);
        }
        if (actual !== expected) {
            throw new Error(
                `node "${id}" is a ${actual}, expected a ${expected}`,
            );
        }
    };

    /** An input read at the coordinate `uv` of the function it appears in. */
    const resolveInFunction = (
        value: VideoValue,
        expected: VideoValueType,
        natural = false,
    ): string => {
        if (value.kind !== 'node') return resolve(value, expected, natural);
        checkNodeType(value.id, expected);
        defineFunction(value.id);
        const call = `f${records.get(value.id)!.index}(uv)`;
        return natural ? `(${call} * 5.0)` : call;
    };

    /** An input handed to a module as a function of coordinates. */
    const resolveAsFunction = (
        value: VideoValue,
        expected: VideoValueType,
    ): string => {
        if (value.kind === 'node') {
            checkNodeType(value.id, expected);
            defineFunction(value.id);
            return `f${records.get(value.id)!.index}`;
        }
        const expression = resolve(value, expected);
        const key = `${expected}:${expression}`;
        let name = constantFunctions.get(key);
        if (name === undefined) {
            name = `k${constantFunctions.size}`;
            constantFunctions.set(key, name);
            functions.push(
                `fn ${name}(uv: vec2f) -> ${wgslType(expected)} {\n    return ${expression};\n}`,
            );
        }
        return name;
    };

    function defineFunction(id: string): void {
        if (functionsDone.has(id)) return;
        functionsDone.add(id);
        const { index, node, def, params, indices } = records.get(id)!;
        const args: Record<string, string> = {};
        for (const [name, type] of Object.entries(def.inputs)) {
            const value = node.inputs[name];
            args[name] = def.warped?.includes(name)
                ? resolveAsFunction(value, type)
                : resolveInFunction(
                      value,
                      type,
                      def.natural?.includes(name) ?? false,
                  );
        }
        functions.push(
            `fn f${index}(uv: vec2f) -> ${wgslType(def.output)} {\n    return ${def.emit(args, params, indices)};\n}`,
        );
    }

    graph.nodes.forEach((node, index) => {
        const def = VIDEO_MODULES[node.kind];
        if (def === undefined) {
            throw new Error(`unknown video module "${node.kind}"`);
        }
        if (types.has(node.id)) {
            throw new Error(`duplicate node id "${node.id}"`);
        }
        try {
            const args: Record<string, string> = {};
            for (const [name, type] of Object.entries(def.inputs)) {
                const value = node.inputs[name];
                if (value === undefined)
                    throw new Error(`missing input "${name}"`);
                args[name] = def.warped?.includes(name)
                    ? resolveAsFunction(value, type)
                    : resolve(
                          value,
                          type,
                          def.natural?.includes(name) ?? false,
                      );
            }
            for (const name of Object.keys(node.inputs)) {
                if (!(name in def.inputs))
                    throw new Error(`unknown input "${name}"`);
            }
            const params: Record<string, string> = {};
            for (const [name, spec] of Object.entries(def.params)) {
                const chosen = node.params?.[name] ?? spec.default;
                if (!spec.values.includes(chosen)) {
                    throw new Error(
                        `param "${name}" must be one of ${spec.values.join(', ')}, got "${chosen}"`,
                    );
                }
                params[name] = chosen;
            }
            for (const name of Object.keys(node.params ?? {})) {
                if (!(name in def.params))
                    throw new Error(`unknown param "${name}"`);
            }
            if (def.history === undefined) {
                if (node.history !== undefined) {
                    throw new Error('module has no audio history');
                }
            } else if (
                node.history === undefined ||
                !Number.isInteger(node.history) ||
                node.history < 0 ||
                node.history >= graph.histories.length
            ) {
                throw new Error(
                    `audio history row must be an integer from 0 to ${graph.histories.length - 1}`,
                );
            }
            if (def.source === undefined) {
                if (node.source !== undefined) {
                    throw new Error('module has no media source');
                }
            } else if (
                node.source === undefined ||
                !Number.isInteger(node.source) ||
                node.source < 0 ||
                node.source >= graph.sources.length
            ) {
                throw new Error(
                    `media source must be an integer from 0 to ${graph.sources.length - 1}`,
                );
            }
            if (def.buffer === undefined) {
                if (node.buffer !== undefined) {
                    throw new Error('module has no feedback buffer');
                }
            } else if (
                node.buffer === undefined ||
                !Number.isInteger(node.buffer) ||
                node.buffer < 0 ||
                node.buffer >= MAX_FEEDBACK_BUFFERS
            ) {
                throw new Error(
                    `feedback buffer must be an integer from 0 to ${MAX_FEEDBACK_BUFFERS - 1}`,
                );
            }
            for (const helper of def.helpers ?? []) helpers.add(helper);
            const local = `v${index}`;
            const indices = {
                buffer: node.buffer ?? 0,
                history: node.history ?? 0,
                source: node.source ?? 0,
            };
            records.set(node.id, { def, index, indices, node, params });
            const text = `    let ${local}: ${wgslType(def.output)} = ${def.emit(args, params, indices)};`;
            lines.push(text);
            statements.push({
                id: node.id,
                reads: Object.values(node.inputs).flatMap((v) =>
                    v.kind === 'node' ? [v.id] : [],
                ),
                text,
            });
            if (node.buffer !== undefined) {
                bufferCount = Math.max(bufferCount, node.buffer + 1);
                if (def.buffer === 'write') {
                    if (bufferWrites.has(node.buffer)) {
                        throw new Error(
                            `feedback buffer ${node.buffer} is written twice`,
                        );
                    }
                    bufferWrites.set(node.buffer, local);
                }
            }
            types.set(node.id, def.output);
            names.set(node.id, local);
        } catch (error) {
            const message =
                error instanceof Error ? error.message : String(error);
            throw new Error(
                `video node "${node.id}" (${node.kind}): ${message}`,
                { cause: error },
            );
        }
    });

    const outputType = types.get(graph.output);
    if (outputType !== 'color') {
        throw new Error(`video output "${graph.output}" must be a color node`);
    }

    for (let buffer = 0; buffer < bufferCount; buffer++) {
        if (!bufferWrites.has(buffer)) {
            throw new Error(
                `feedback buffer ${buffer} is read but never written`,
            );
        }
    }

    const previewEntries = graph.previews.map((preview, k) => {
        const expression = resolve(preview.value, preview.type);
        const needed = new Set<string>(
            preview.value.kind === 'node' ? [preview.value.id] : [],
        );
        for (let i = statements.length - 1; i >= 0; i--) {
            if (needed.has(statements[i].id)) {
                statements[i].reads.forEach((id) => needed.add(id));
            }
        }
        const body = statements
            .filter((s) => needed.has(s.id))
            .map((s) => s.text)
            .join('\n');
        const color =
            preview.type === 'color' ? expression : `vec3f(${expression})`;
        return `
@fragment
fn preview_${k}(@builtin(position) frag: vec4f) -> @location(0) vec4f {
    let uv = vec2f(frag.x / u.resolution.x, 1.0 - frag.y / u.resolution.y);
${body}
    return vec4f(${color}, 1.0);
}
`;
    });

    const slotVecs = Math.max(1, Math.ceil(graph.uniforms.length / 4));
    const slots = bindingSlots(bufferCount, graph.histories.length);
    const samplerDeclaration =
        bufferCount > 0 || graph.sources.length > 0
            ? `@group(0) @binding(${slots.sampler}) var fb_sampler: sampler;\n`
            : '';
    const bufferDeclarations = Array.from(
        { length: bufferCount },
        (_, k) =>
            `@group(0) @binding(${slots.buffer + k}) var fb_${k}: texture_2d<f32>;\n`,
    ).join('');
    const historyDeclaration =
        graph.histories.length === 0
            ? ''
            : `@group(0) @binding(${slots.history}) var history_tex: texture_2d<f32>;\n`;
    const sourceDeclarations = graph.sources
        .map(
            (_, k) =>
                `@group(0) @binding(${slots.source + k}) var src_${k}: texture_2d<f32>;\n`,
        )
        .join('');
    const outputStruct =
        bufferCount === 0
            ? ''
            : `
struct FragOut {
    @location(0) screen: vec4f,
${Array.from({ length: bufferCount }, (_, k) => `    @location(${k + 1}) fb${k}: vec4f,`).join('\n')}
}
`;
    const result =
        bufferCount === 0
            ? `vec4f(${names.get(graph.output)}, 1.0)`
            : `FragOut(vec4f(${names.get(graph.output)}, 1.0), ${Array.from({ length: bufferCount }, (_, k) => `vec4f(${bufferWrites.get(k)}, 1.0)`).join(', ')})`;
    const wgsl = `struct Uniforms {
    time: f32,
    resolution: vec2f,
    slots: array<vec4f, ${slotVecs}>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
${samplerDeclaration}${bufferDeclarations}${historyDeclaration}${sourceDeclarations}${outputStruct}${[...helpers].map((h) => `\n${h}\n`).join('')}${functions.map((f) => `\n${f}\n`).join('')}
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) frag: vec4f) -> ${bufferCount === 0 ? '@location(0) vec4f' : 'FragOut'} {
    let uv = vec2f(frag.x / u.resolution.x, 1.0 - frag.y / u.resolution.y);
${lines.join('\n')}
    return ${result};
}
${previewEntries.join('')}`;
    return {
        wgsl,
        uniformFloatCount: UNIFORM_SLOTS_OFFSET + slotVecs * 4,
        uniforms: graph.uniforms,
        feedbackBufferCount: bufferCount,
        histories: graph.histories,
        sources: graph.sources,
        hasOutput: graph.hasOutput,
        previewCount: graph.previews.length,
        cvSamples: graph.previews.flatMap((preview, index) =>
            preview.cv ? [{ index, ...preview.cv }] : [],
        ),
    };
}
