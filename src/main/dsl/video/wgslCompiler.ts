import type {
    VideoGraph,
    VideoValue,
    VideoValueType,
} from '../../../shared/video/videoGraph';
import { VIDEO_MODULES } from './modules';

/** Floats before the slot array in the uniform buffer: time, pad, resolution. */
export const UNIFORM_HEADER_FLOATS = 4;

export interface CompiledVideoShader {
    wgsl: string;
    /** Total uniform buffer size in floats, a multiple of 4. */
    uniformFloatCount: number;
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

    const resolve = (value: VideoValue, expected: VideoValueType): string => {
        switch (value.kind) {
            case 'const':
            case 'uniform':
            case 'time':
                if (expected !== 'field') {
                    throw new Error(
                        `expected a ${expected}, got a ${value.kind}`,
                    );
                }
                if (value.kind === 'const') return wgslFloat(value.value);
                if (value.kind === 'time') return 'u.time';
                if (
                    !Number.isInteger(value.slot) ||
                    value.slot < 0 ||
                    value.slot >= graph.uniformSlotCount
                ) {
                    throw new Error(
                        `uniform slot ${value.slot} is out of range`,
                    );
                }
                return `u.slots[${value.slot >> 2}][${value.slot & 3}]`;
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
                return names.get(value.id)!;
            }
        }
    };

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
                args[name] = resolve(value, type);
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
            const local = `v${index}`;
            const wgslType = def.output === 'field' ? 'f32' : 'vec3f';
            lines.push(
                `    let ${local}: ${wgslType} = ${def.emit(args, params)};`,
            );
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

    const slotVecs = Math.max(1, Math.ceil(graph.uniformSlotCount / 4));
    const wgsl = `struct Uniforms {
    time: f32,
    resolution: vec2f,
    slots: array<vec4f, ${slotVecs}>,
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
${lines.join('\n')}
    return vec4f(${names.get(graph.output)}, 1.0);
}
`;
    return { wgsl, uniformFloatCount: UNIFORM_HEADER_FLOATS + slotVecs * 4 };
}
