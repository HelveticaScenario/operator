import {
    MAX_FEEDBACK_BUFFERS,
    type CompiledVideoShader,
    type VideoGraph,
    type VideoInputKind,
    type VideoValue,
} from '../../../shared/video/videoGraph';
import {
    UNIFORM_SLOTS_OFFSET,
    bindingSlots,
} from '../../../shared/video/uniformLayout';
import { VIDEO_MODULES } from './modules';
import { gray, LUMA_HELPER } from './modules/rgba';
import type { VideoModuleDef } from './modules/types';

/**
 * `video_display`: what the screen shows of a color, its channels clipped to
 * 0..1 and laid over black by its alpha.
 */
const DISPLAY_HELPER = `fn video_display(c: vec4f) -> vec4f {
    let clipped = clamp(c, vec4f(0.0), vec4f(1.0));
    return vec4f(clipped.rgb * clipped.a, 1.0);
}`;

/** The WGSL of a module's value, as an RGBA `vec4f`. */
function emitRgba(
    def: VideoModuleDef,
    ...args: Parameters<VideoModuleDef['emit']>
): string {
    const value = def.emit(...args);
    return def.output === 'gray' ? gray(value) : value;
}

function wgslFloat(value: number): string {
    if (!Number.isFinite(Math.fround(value))) {
        throw new Error(
            `video constant must be finite and within the f32 range, got ${value}`,
        );
    }
    const text = String(value);
    return /^-?\d+$/.test(text) ? `${text}.0` : text;
}

/**
 * Most node evaluations a pixel may cost. A warped input re-evaluates its whole
 * sub-patch per sample, so nesting multiplies: blurring an oscillator twice
 * costs about 570 and three times about 9000, which can stall the GPU long
 * enough for the system to reset it.
 */
const MAX_EVALUATIONS_PER_PIXEL = 2048;

/** How many times `def.emit` writes each input into its expression. */
function inputUses(
    def: VideoModuleDef,
    params: Record<string, string>,
    indices: Parameters<VideoModuleDef['emit']>[2],
): Map<string, number> {
    const token = (name: string) => `\u0000${name}\u0000`;
    const text = def.emit(
        Object.fromEntries(
            Object.keys(def.inputs).map((name) => [name, token(name)]),
        ),
        params,
        indices,
    );
    return new Map(
        Object.keys(def.inputs).map((name) => [
            name,
            text.split(token(name)).length - 1,
        ]),
    );
}

/**
 * Checks a node's index into a resource: present and in `0..count` when its
 * module `uses` the resource, absent otherwise.
 */
function checkIndex(
    uses: boolean,
    index: number | undefined,
    count: number,
    resource: string,
    label: string,
): void {
    if (!uses) {
        if (index !== undefined) throw new Error(`module has no ${resource}`);
        return;
    }
    if (
        index === undefined ||
        !Number.isInteger(index) ||
        index < 0 ||
        index >= count
    ) {
        throw new Error(`${label} must be an integer from 0 to ${count - 1}`);
    }
}

/**
 * Compiles a video graph to one WGSL module with a fullscreen-triangle vertex
 * stage and a fragment stage that evaluates every node per pixel.
 */
export function compileVideoGraph(graph: VideoGraph): CompiledVideoShader {
    const names = new Map<string, string>();
    const helpers = new Set<string>();
    /** Each node's WGSL statement, with the nodes it reads, in graph order. */
    const statements: { id: string; text: string; reads: string[] }[] = [];
    /** Local variable holding what each feedback buffer stores this frame. */
    const bufferWrites = new Map<number, string>();
    let bufferCount = 0;

    /**
     * Each node as a function of the coordinate, `fn f<index>(uv) -> vec4f`, for
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
    /** Evaluations one call of each node's coordinate function costs, its inputs' included. */
    const callCosts = new Map<string, number>();
    let pixelCost = 0;
    const functions: string[] = [];
    const functionsDone = new Set<string>();
    const constantFunctions = new Map<string, string>();
    const checkNode = (id: string) => {
        if (!names.has(id)) throw new Error(`unknown or forward node "${id}"`);
    };

    /** A node's color, `rgba`, read whole or as its luma. */
    const readColor = (color: string, kind: VideoInputKind): string => {
        if (kind === 'rgba') return color;
        helpers.add(LUMA_HELPER);
        return `video_luma(${color})`;
    };

    /** The `f32` WGSL of an input that is not a node. */
    const number = (value: Exclude<VideoValue, { kind: 'node' }>): string => {
        if (value.kind === 'const') return wgslFloat(value.value);
        if (
            value.kind === 'uniform' &&
            !(
                Number.isInteger(value.slot) &&
                value.slot >= 0 &&
                value.slot < graph.uniforms.length
            )
        ) {
            throw new Error(`uniform slot ${value.slot} is out of range`);
        }
        return value.kind === 'time'
            ? 'u.time'
            : `u.slots[${value.slot >> 2}][${value.slot & 3}]`;
    };

    /** The WGSL for an input. */
    const resolve = (value: VideoValue, kind: VideoInputKind): string => {
        if (value.kind === 'node') {
            checkNode(value.id);
            return readColor(names.get(value.id)!, kind);
        }
        const level = number(value);
        return kind === 'number' ? level : gray(level);
    };

    /** An input read at the coordinate `uv` of the function it appears in. */
    const resolveInFunction = (
        value: VideoValue,
        kind: VideoInputKind,
    ): string => {
        if (value.kind !== 'node') return resolve(value, kind);
        checkNode(value.id);
        defineFunction(value.id);
        return readColor(`f${records.get(value.id)!.index}(uv)`, kind);
    };

    /** An input handed to a module as a function from coordinates to its color. */
    const resolveAsFunction = (value: VideoValue): string => {
        if (value.kind === 'node') {
            checkNode(value.id);
            defineFunction(value.id);
            return `f${records.get(value.id)!.index}`;
        }
        const expression = resolve(value, 'rgba');
        let name = constantFunctions.get(expression);
        if (name === undefined) {
            name = `k${constantFunctions.size}`;
            constantFunctions.set(expression, name);
            functions.push(
                `fn ${name}(uv: vec2f) -> vec4f {\n    return ${expression};\n}`,
            );
        }
        return name;
    };

    /**
     * The WGSL of each input of `node`: warped inputs as functions of the
     * coordinate, the rest through `read`.
     */
    function argsOf(
        node: VideoGraph['nodes'][number],
        def: VideoModuleDef,
        read: typeof resolve,
    ): Record<string, string> {
        const args: Record<string, string> = {};
        for (const [name, kind] of Object.entries(def.inputs)) {
            const value = node.inputs[name];
            if (value === undefined) throw new Error(`missing input "${name}"`);
            args[name] = def.warped?.includes(name)
                ? resolveAsFunction(value)
                : read(value, kind);
        }
        return args;
    }

    function defineFunction(id: string): void {
        if (functionsDone.has(id)) return;
        functionsDone.add(id);
        const { index, node, def, params, indices } = records.get(id)!;
        const args = argsOf(node, def, resolveInFunction);
        functions.push(
            `fn f${index}(uv: vec2f) -> vec4f {\n    return ${emitRgba(def, args, params, indices)};\n}`,
        );
    }

    graph.nodes.forEach((node, index) => {
        const def = VIDEO_MODULES[node.kind];
        if (def === undefined) {
            throw new Error(`unknown video module "${node.kind}"`);
        }
        if (names.has(node.id)) {
            throw new Error(`duplicate node id "${node.id}"`);
        }
        try {
            const args = argsOf(node, def, resolve);
            for (const name of Object.keys(node.inputs)) {
                if (!(name in def.inputs))
                    throw new Error(`unknown input "${name}"`);
            }
            const params: Record<string, string> = {};
            for (const [name, spec] of Object.entries(def.params ?? {})) {
                const chosen = node.params?.[name] ?? spec.default;
                if (!spec.values.includes(chosen)) {
                    throw new Error(
                        `param "${name}" must be one of ${spec.values.join(', ')}, got "${chosen}"`,
                    );
                }
                params[name] = chosen;
            }
            for (const name of Object.keys(node.params ?? {})) {
                if (!(name in (def.params ?? {})))
                    throw new Error(`unknown param "${name}"`);
            }
            checkIndex(
                def.history !== undefined,
                node.history,
                graph.histories.length,
                'audio history',
                'audio history row',
            );
            checkIndex(
                def.source !== undefined,
                node.source,
                graph.sources.length,
                'media source',
                'media source',
            );
            checkIndex(
                def.buffer !== undefined,
                node.buffer,
                MAX_FEEDBACK_BUFFERS,
                'feedback buffer',
                'feedback buffer',
            );
            const needed =
                typeof def.helpers === 'function'
                    ? def.helpers(params)
                    : (def.helpers ?? []);
            for (const helper of needed) helpers.add(helper);
            const local = `v${index}`;
            const indices = {
                buffer: node.buffer ?? 0,
                history: node.history ?? 0,
                source: node.source ?? 0,
            };
            records.set(node.id, { def, index, indices, node, params });
            let callCost = 1;
            let statementCost = 1;
            for (const [name, uses] of inputUses(def, params, indices)) {
                const value = node.inputs[name];
                if (value.kind !== 'node') continue;
                const inputCost = uses * callCosts.get(value.id)!;
                callCost += inputCost;
                if (def.warped?.includes(name)) statementCost += inputCost;
            }
            callCosts.set(node.id, callCost);
            pixelCost += statementCost;
            const text = `    let ${local}: vec4f = ${emitRgba(def, args, params, indices)};`;
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

    if (!names.has(graph.output)) {
        throw new Error(`video output "${graph.output}" is not a node`);
    }
    if (pixelCost > MAX_EVALUATIONS_PER_PIXEL) {
        throw new Error(
            `$v: the picture costs ${pixelCost} evaluations per pixel, more than the ${MAX_EVALUATIONS_PER_PIXEL} the GPU can draw in time; blur, bloom, edges and the warps re-evaluate their whole input at each coordinate they read, so nesting them multiplies the cost`,
        );
    }
    helpers.add(DISPLAY_HELPER);

    for (let buffer = 0; buffer < bufferCount; buffer++) {
        if (!bufferWrites.has(buffer)) {
            throw new Error(
                `feedback buffer ${buffer} is read but never written`,
            );
        }
    }

    // A preview keeps the signal's own alpha, so the editor can show its
    // transparency; whatever reads it as a picture lays it over black.
    const previewEntries = graph.previews.map((preview, k) => {
        const expression = resolve(preview.value, 'rgba');
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
        return `
@fragment
fn preview_${k}(@builtin(position) frag: vec4f) -> @location(0) vec4f {
    let uv = vec2f(frag.x / u.resolution.x, 1.0 - frag.y / u.resolution.y);
${body}
    return clamp(${expression}, vec4f(0.0), vec4f(1.0));
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
            ? `video_display(${names.get(graph.output)})`
            : `FragOut(video_display(${names.get(graph.output)}), ${Array.from({ length: bufferCount }, (_, k) => bufferWrites.get(k)).join(', ')})`;
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
${statements.map((s) => s.text).join('\n')}
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
