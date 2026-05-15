import type { PatchGraph } from '../shared/ipcTypes';

type ModuleState = PatchGraph['modules'][number];

const RESERVED_MODULE_IDS = new Set([
    'ROOT_OUTPUT',
    'ROOT_CLOCK',
    'ROOT_INPUT',
    'HIDDEN_AUDIO_IN',
]);

const DEFAULT_MATCH_THRESHOLD = 0.65;
const DEFAULT_AMBIGUITY_MARGIN = 0.05;

function escapeRegExp(value: string): string {
    return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function isLikelyImplicitId(id: string, moduleType: string): boolean {
    // DSL-generated ids are typically `${moduleType}-${counter}`.
    // Treat those as "implicit" so they do not create strong identity across edits.
    const re = new RegExp(`^${escapeRegExp(moduleType)}-\\d+$`);
    return re.test(id);
}

function isExplicitId(module: Pick<ModuleState, 'id' | 'moduleType'>): boolean {
    // Prefer the real DSL-provided flag when present.
    // (napi-rs typically maps Option<bool> to `boolean | null | undefined` in TS)
    const maybeFlag = (module as unknown as { idIsExplicit?: boolean | null })
        .idIsExplicit;
    if (typeof maybeFlag === 'boolean') {
        return maybeFlag;
    }

    // Back-compat fallback for older graphs.
    if (RESERVED_MODULE_IDS.has(module.id)) {
        return true;
    }
    return !isLikelyImplicitId(module.id, module.moduleType);
}

function deepClone<T>(value: T): T {
    // PatchGraph is plain JSON-ish data; JSON clone is fine and avoids structuredClone
    // Availability differences between Electron main/renderer build targets.
    return JSON.parse(JSON.stringify(value)) as T;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isCableRef(
    value: unknown,
): value is { type: 'cable'; module: string; port: string } {
    if (!isPlainObject(value)) {
        return false;
    }
    return (
        value.type === 'cable' &&
        typeof value.module === 'string' &&
        typeof value.port === 'string'
    );
}

function isBufferRef(value: unknown): value is {
    type: 'buffer_ref';
    module: string;
    port: string;
    channels: number;
    frameCount: number;
} {
    if (!isPlainObject(value)) {
        return false;
    }
    return (
        value.type === 'buffer_ref' &&
        typeof value.module === 'string' &&
        typeof value.port === 'string' &&
        typeof value.frameCount === 'number'
    );
}

function walkValues(
    value: unknown,
    visit: (value: unknown, path: string) => void,
    path: string = '',
): void {
    visit(value, path);

    if (Array.isArray(value)) {
        for (let i = 0; i < value.length; i++) {
            walkValues(value[i], visit, path ? `${path}[${i}]` : `[${i}]`);
        }
        return;
    }

    if (!isPlainObject(value)) {
        return;
    }

    const keys = Object.keys(value).sort();
    for (const key of keys) {
        const nextPath = path ? `${path}.${key}` : key;
        walkValues(value[key], visit, nextPath);
    }
}

type FeatureKind =
    | 'number'
    | 'boolean'
    | 'string'
    | 'null'
    | 'cableRef'
    | 'bufferRef'
    | 'unknown';

interface Feature {
    key: string;
    kind: FeatureKind;
    value: unknown;
    weight: number;
}

function kindWeight(kind: FeatureKind): number {
    switch (kind) {
        case 'cableRef':
        case 'bufferRef':
            return 2.0;
        case 'number':
            return 1.0;
        case 'boolean':
        case 'string':
        case 'null':
            return 1.0;

        default:
            return 0.75;
    }
}

function moduleTypeById(graph: PatchGraph): Map<string, string> {
    const map = new Map<string, string>();
    for (const m of graph.modules) {
        map.set(m.id, m.moduleType);
    }
    return map;
}

interface GraphContext {
    typeById: Map<string, string>;
    featuresByModuleId: Map<string, Map<string, Feature>>;
}

function _canonicalizeForFingerprint(
    value: unknown,
    ctx: { typeById: Map<string, string> },
): unknown {
    if (isCableRef(value)) {
        const upstreamType = ctx.typeById.get(value.module) ?? 'unknown';
        return { port: value.port, type: 'cable', upstreamType };
    }

    if (isBufferRef(value)) {
        const upstreamType = ctx.typeById.get(value.module) ?? 'unknown';
        return {
            channels: value.channels,
            frameCount: value.frameCount,
            port: value.port,
            type: 'buffer_ref',
            upstreamType,
        };
    }

    if (Array.isArray(value)) {
        return value.map((v) => _canonicalizeForFingerprint(v, ctx));
    }
    if (isPlainObject(value)) {
        const out: Record<string, unknown> = {};
        const keys = Object.keys(value).sort();
        for (const k of keys) {
            out[k] = _canonicalizeForFingerprint(value[k], ctx);
        }
        return out;
    }
    return value;
}

function extractFeatures(
    ctx: Pick<GraphContext, 'typeById'>,
    module: ModuleState,
): Map<string, Feature> {
    const features = new Map<string, Feature>();

    const walk = (v: unknown, path: string) => {
        const key = path || '$';

        if (isCableRef(v)) {
            const upstreamType = ctx.typeById.get(v.module) ?? 'unknown';
            const canonical = `${upstreamType}:${v.port}`;
            features.set(key, {
                key,
                kind: 'cableRef',
                value: canonical,
                weight: kindWeight('cableRef'),
            });
            return; // Treat as leaf
        }

        if (isBufferRef(v)) {
            const upstreamType = ctx.typeById.get(v.module) ?? 'unknown';
            const canonical = `${upstreamType}:${v.port}:${v.channels}:${v.frameCount}`;
            features.set(key, {
                key,
                kind: 'bufferRef',
                value: canonical,
                weight: kindWeight('bufferRef'),
            });
            return; // Treat as leaf
        }

        if (typeof v === 'number') {
            features.set(key, {
                key,
                kind: 'number',
                value: v,
                weight: kindWeight('number'),
            });
            return;
        }
        if (typeof v === 'boolean') {
            features.set(key, {
                key,
                kind: 'boolean',
                value: v,
                weight: kindWeight('boolean'),
            });
            return;
        }
        if (typeof v === 'string') {
            features.set(key, {
                key,
                kind: 'string',
                value: v,
                weight: kindWeight('string'),
            });
            return;
        }
        if (v === null) {
            features.set(key, {
                key,
                kind: 'null',
                value: null,
                weight: kindWeight('null'),
            });
            return;
        }

        if (Array.isArray(v)) {
            for (let i = 0; i < v.length; i++) {
                walk(v[i], path ? `${path}[${i}]` : `[${i}]`);
            }
            return;
        }

        if (isPlainObject(v)) {
            const keys = Object.keys(v).sort();
            for (const k of keys) {
                walk(v[k], path ? `${path}.${k}` : k);
            }
        }
    };

    walk(module.params, '');

    return features;
}

function buildGraphContext(graph: PatchGraph): GraphContext {
    const typeById = moduleTypeById(graph);
    const ctxForFeatures = { typeById };

    const featuresByModuleId = new Map<string, Map<string, Feature>>();
    for (const module of graph.modules) {
        featuresByModuleId.set(
            module.id,
            extractFeatures(ctxForFeatures, module),
        );
    }

    return { featuresByModuleId, typeById };
}

function multisetAdd(map: Map<string, number>, key: string): void {
    map.set(key, (map.get(key) ?? 0) + 1);
}

function computeDownstreamUsage(
    graph: PatchGraph,
): Map<string, Map<string, number>> {
    const usage = new Map<string, Map<string, number>>();

    const record = (producerId: string, token: string) => {
        const bag = usage.get(producerId) ?? new Map<string, number>();
        multisetAdd(bag, token);
        usage.set(producerId, bag);
    };

    const moduleType = new Map<string, string>();
    for (const m of graph.modules) {
        moduleType.set(m.id, m.moduleType);
    }

    for (const consumer of graph.modules) {
        walkValues(consumer.params, (v, path) => {
            if (isCableRef(v)) {
                const consumerType = consumer.moduleType;
                const token = `${consumerType}:${path}:${v.port}`;
                record(v.module, token);
                return;
            }
            if (isBufferRef(v)) {
                const consumerType = consumer.moduleType;
                const token = `${consumerType}:${path}:${v.port}`;
                record(v.module, token);
                return;
            }
        });
    }

    return usage;
}

function numberSimilarity(a: number, b: number): number {
    const denom = Math.abs(a) + Math.abs(b) + 1e-6;
    const rel = Math.abs(a - b) / denom;
    return 1 - Math.min(1, rel);
}

function featureSimilarity(
    a: Feature | undefined,
    b: Feature | undefined,
): { score: number; weight: number } {
    if (!a || !b) {
        const w = (a?.weight ?? 0) + (b?.weight ?? 0);
        return { score: 0, weight: Math.max(1e-6, w) };
    }

    if (a.kind !== b.kind) {
        return { score: 0, weight: (a.weight + b.weight) / 2 };
    }

    const w = (a.weight + b.weight) / 2;

    switch (a.kind) {
        case 'number':
            return {
                score: numberSimilarity(a.value as number, b.value as number),
                weight: w,
            };
        case 'boolean':
        case 'string':
        case 'null':
        case 'cableRef':
        case 'bufferRef':
            return { score: a.value === b.value ? 1 : 0, weight: w };
        default:
            return {
                score:
                    JSON.stringify(a.value) === JSON.stringify(b.value) ? 1 : 0,
                weight: w,
            };
    }
}

function paramSimilarity(
    featuresA: Map<string, Feature>,
    featuresB: Map<string, Feature>,
): number {
    const keys = new Set<string>();
    for (const k of featuresA.keys()) {
        keys.add(k);
    }
    for (const k of featuresB.keys()) {
        keys.add(k);
    }

    let weightedSum = 0;
    let weightTotal = 0;

    for (const key of keys) {
        const { score, weight } = featureSimilarity(
            featuresA.get(key),
            featuresB.get(key),
        );
        weightedSum += score * weight;
        weightTotal += weight;
    }

    if (weightTotal <= 0) {
        return 0;
    }
    return weightedSum / weightTotal;
}

function multisetJaccard(
    a: Map<string, number>,
    b: Map<string, number>,
): number {
    const keys = new Set<string>();
    for (const k of a.keys()) {
        keys.add(k);
    }
    for (const k of b.keys()) {
        keys.add(k);
    }

    let minSum = 0;
    let maxSum = 0;
    for (const key of keys) {
        const av = a.get(key) ?? 0;
        const bv = b.get(key) ?? 0;
        minSum += Math.min(av, bv);
        maxSum += Math.max(av, bv);
    }

    if (maxSum === 0) {
        return 1;
    } // Both empty => identical
    return minSum / maxSum;
}

function moduleSimilarity(
    desiredGraph: PatchGraph,
    desired: ModuleState,
    currentGraph: PatchGraph,
    current: ModuleState,
    desiredDownstream: Map<string, Map<string, number>>,
    currentDownstream: Map<string, Map<string, number>>,
    desiredCtx: GraphContext,
    currentCtx: GraphContext,
): number {
    if (desired.moduleType !== current.moduleType) {
        return 0;
    }

    const desiredFeatures =
        desiredCtx.featuresByModuleId.get(desired.id) ??
        new Map<string, Feature>();
    const currentFeatures =
        currentCtx.featuresByModuleId.get(current.id) ??
        new Map<string, Feature>();
    const pSim = paramSimilarity(desiredFeatures, currentFeatures);

    const dBag = desiredDownstream.get(desired.id) ?? new Map<string, number>();
    const cBag = currentDownstream.get(current.id) ?? new Map<string, number>();
    const downSim = multisetJaccard(dBag, cBag);

    // Weight params slightly more, but include downstream usage to disambiguate clones.
    let base = 0.6 * pSim + 0.4 * downSim;

    // Explicit id bias:
    // - If the desired module id is user-assigned (not the DSL auto `${type}-${n}`),
    //   Strongly prefer matching the same id.
    // - Do NOT create a hard lock for implicit ids, since those are unstable.
    const desiredExplicit = isExplicitId(desired);
    const sameId = desired.id === current.id;

    if (desiredExplicit && sameId) {
        // Strong preference for exact explicit id match.
        base = Math.max(base, 0.99);
    }

    return base;
}

function hungarian(cost: number[][]): number[] {
    // Minimal-cost assignment for a square matrix.
    // Implementation: potentials + augmenting path (O(n^3)).
    const n = cost.length;
    const u = new Array(n + 1).fill(0);
    const v = new Array(n + 1).fill(0);
    const p = new Array(n + 1).fill(0);
    const way = new Array(n + 1).fill(0);

    for (let i = 1; i <= n; i++) {
        p[0] = i;
        let j0 = 0;
        const minv = new Array(n + 1).fill(Number.POSITIVE_INFINITY);
        const used = new Array(n + 1).fill(false);

        do {
            used[j0] = true;
            const i0 = p[j0];
            let delta = Number.POSITIVE_INFINITY;
            let j1 = 0;

            for (let j = 1; j <= n; j++) {
                if (used[j]) {
                    continue;
                }
                const cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                if (cur < minv[j]) {
                    minv[j] = cur;
                    way[j] = j0;
                }
                if (minv[j] < delta) {
                    delta = minv[j];
                    j1 = j;
                }
            }

            for (let j = 0; j <= n; j++) {
                if (used[j]) {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
        } while (p[j0] !== 0);

        do {
            const j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
        } while (j0 !== 0);
    }

    // P[j] = assigned row for column j
    const assignment = new Array(n).fill(-1); // Row -> column
    for (let j = 1; j <= n; j++) {
        if (p[j] > 0) {
            assignment[p[j] - 1] = j - 1;
        }
    }
    return assignment;
}

function remapModuleIdsInValue(
    value: unknown,
    idMap: Map<string, string>,
): unknown {
    if (isCableRef(value)) {
        const remapped = idMap.get(value.module);
        if (!remapped) {
            return value;
        }
        return { ...value, module: remapped };
    }

    if (isBufferRef(value)) {
        const remapped = idMap.get(value.module);
        if (!remapped) {
            return value;
        }
        return { ...value, module: remapped };
    }

    if (Array.isArray(value)) {
        return value.map((v) => remapModuleIdsInValue(v, idMap));
    }

    if (isPlainObject(value)) {
        const out: Record<string, unknown> = {};
        // Avoid Object.entries to keep compatibility with older TS lib targets.
        for (const k in value) {
            if (Object.hasOwn(value, k)) {
                out[k] = remapModuleIdsInValue(value[k], idMap);
            }
        }
        return out;
    }

    return value;
}

function _remapGraph(
    desiredGraph: PatchGraph,
    idMap: Map<string, string>,
): PatchGraph {
    const applied = deepClone(desiredGraph);

    for (const module of applied.modules) {
        const nextId = idMap.get(module.id);
        if (nextId) {
            module.id = nextId;
        }
        module.params = remapModuleIdsInValue(module.params, idMap);
    }

    for (const scope of applied.scopes) {
        for (const channel of scope.channels) {
            const nextId = idMap.get(channel.moduleId);
            if (nextId) {
                channel.moduleId = nextId;
            }
        }
    }

    return applied;
}

export interface ReconcileOptions {
    matchThreshold?: number;
    ambiguityMargin?: number;
    debugLog?: (message: string) => void;
}

export interface ReconcileResult {
    appliedPatch: PatchGraph;
    moduleIdRemap: Record<string, string>;
}

export function reconcilePatchBySimilarity(
    desiredGraph: PatchGraph,
    currentGraph: PatchGraph | null,
    options: ReconcileOptions = {},
): ReconcileResult {
    if (!currentGraph) {
        return {
            appliedPatch: desiredGraph,
            moduleIdRemap: {},
        };
    }

    const matchThreshold = options.matchThreshold ?? DEFAULT_MATCH_THRESHOLD;
    const ambiguityMargin = options.ambiguityMargin ?? DEFAULT_AMBIGUITY_MARGIN;
    const { debugLog } = options;

    const currentById = new Map<string, ModuleState>();
    for (const m of currentGraph.modules) {
        currentById.set(m.id, m);
    }

    const desiredById = new Map<string, ModuleState>();
    for (const m of desiredGraph.modules) {
        desiredById.set(m.id, m);
    }

    const usedCurrentIds = new Set<string>();

    // 1) Seed mappings only for truly reserved ids, plus exact matches for explicit ids.
    // We deliberately do NOT treat implicit ids (`${type}-${n}`) as stable identity.
    const idMap = new Map<string, string>(); // CURRENT id -> DESIRED id
    const anchoredDesiredIds = new Set<string>();

    // Reserved ids are always stable.
    for (const reservedId of RESERVED_MODULE_IDS) {
        if (currentById.has(reservedId) && desiredById.has(reservedId)) {
            idMap.set(reservedId, reservedId);
            usedCurrentIds.add(reservedId);
            anchoredDesiredIds.add(reservedId);
        }
    }

    // If the desired module has an explicit id and exists in current with same id+type,
    // Anchor it early.
    for (const desired of desiredGraph.modules) {
        if (RESERVED_MODULE_IDS.has(desired.id)) {
            continue;
        }
        if (!isExplicitId(desired)) {
            continue;
        }

        const current = currentById.get(desired.id);
        if (current && current.moduleType === desired.moduleType) {
            idMap.set(current.id, desired.id);
            usedCurrentIds.add(current.id);
            anchoredDesiredIds.add(desired.id);
        }
    }

    const desiredDownstream = computeDownstreamUsage(desiredGraph);
    const currentDownstream = computeDownstreamUsage(currentGraph);

    const desiredCtx = buildGraphContext(desiredGraph);
    const currentCtx = buildGraphContext(currentGraph);

    // 2) Per-type optimal assignment for remaining modules.

    const desiredByType = new Map<string, ModuleState[]>();
    const currentByType = new Map<string, ModuleState[]>();

    for (const desired of desiredGraph.modules) {
        if (RESERVED_MODULE_IDS.has(desired.id)) {
            continue;
        }
        // Skip desired ids already anchored via explicit exact match.
        if (anchoredDesiredIds.has(desired.id)) {
            continue;
        }
        const list = desiredByType.get(desired.moduleType) ?? [];
        list.push(desired);
        desiredByType.set(desired.moduleType, list);
    }

    for (const current of currentGraph.modules) {
        if (usedCurrentIds.has(current.id)) {
            continue;
        }
        if (RESERVED_MODULE_IDS.has(current.id)) {
            continue;
        }
        const list = currentByType.get(current.moduleType) ?? [];
        list.push(current);
        currentByType.set(current.moduleType, list);
    }

    for (const [moduleType, desiredList] of desiredByType.entries()) {
        const currentList = currentByType.get(moduleType) ?? [];

        const m = desiredList.length;
        const n = currentList.length;
        if (m === 0 || n === 0) {
            continue;
        }

        // Cost matrix: size x size, where size = n + m (adds one dummy column per desired)
        const size = n + m;
        const thresholdCost = 1 - matchThreshold;

        const cost: number[][] = Array.from({ length: size }, () =>
            new Array(size).fill(0),
        );
        const scoreMatrix: number[][] = Array.from({ length: m }, () =>
            new Array(n).fill(0),
        );

        for (let i = 0; i < m; i++) {
            for (let j = 0; j < n; j++) {
                const score = moduleSimilarity(
                    desiredGraph,
                    desiredList[i],
                    currentGraph,
                    currentList[j],
                    desiredDownstream,
                    currentDownstream,
                    desiredCtx,
                    currentCtx,
                );
                scoreMatrix[i][j] = score;
                cost[i][j] = 1 - score;
            }
            // Dummy columns => "no match"
            for (let j = n; j < size; j++) {
                cost[i][j] = thresholdCost;
            }
        }

        // Dummy rows to make the matrix square / allow unused columns.
        // IMPORTANT: use a constant non-zero-ish cost here (not 0) to avoid
        // Degeneracy in large matrices where dummy rows can "steal" real columns
        // And force desired rows onto dummy columns.
        for (let i = m; i < size; i++) {
            for (let j = 0; j < size; j++) {
                cost[i][j] = thresholdCost;
            }
        }

        const assignment = hungarian(cost);

        for (let i = 0; i < m; i++) {
            const assignedCol = assignment[i];
            if (assignedCol < 0 || assignedCol >= n) {
                continue;
            } // Dummy => unmatched

            const score = scoreMatrix[i][assignedCol];
            // Ambiguity guard: best must beat second-best by margin.
            const row = scoreMatrix[i];
            let best = -1;
            let second = -1;
            for (const s of row) {
                if (s > best) {
                    second = best;
                    best = s;
                } else if (s > second) {
                    second = s;
                }
            }
            const margin = best - second;

            const desiredId = desiredList[i].id;
            const currentId = currentList[assignedCol].id;

            const desiredExplicit = isExplicitId(desiredList[i]);
            const currentExplicit = isExplicitId(currentList[assignedCol]);

            if (debugLog) {
                debugLog(
                    `[patch-remap] type=${moduleType} desired=${desiredId} candidate=${currentId} score=${score.toFixed(
                        4,
                    )} best=${best.toFixed(4)} second=${second.toFixed(4)} margin=${margin.toFixed(
                        4,
                    )} desiredExplicit=${desiredExplicit} currentExplicit=${currentExplicit} sameId=${desiredId === currentId}`,
                );
            }

            if (score < matchThreshold) {
                if (debugLog) {
                    debugLog(
                        `[patch-remap] reject (below-threshold) desired=${desiredId} score=${score.toFixed(
                            4,
                        )} threshold=${matchThreshold.toFixed(4)}`,
                    );
                }
                continue;
            }

            if (margin < ambiguityMargin) {
                if (debugLog) {
                    debugLog(
                        `[patch-remap] reject (ambiguous) desired=${desiredId} best=${best.toFixed(
                            4,
                        )} second=${second.toFixed(4)} margin=${margin.toFixed(
                            4,
                        )} required=${ambiguityMargin.toFixed(4)}`,
                    );
                }
                continue;
            }

            // Never remap onto a reserved id.
            if (RESERVED_MODULE_IDS.has(currentId)) {
                continue;
            }
            if (RESERVED_MODULE_IDS.has(desiredId)) {
                continue;
            }

            idMap.set(currentId, desiredId);
            usedCurrentIds.add(currentId);

            if (debugLog) {
                debugLog(
                    `[patch-remap] accept type=${moduleType} ${currentId} -> ${desiredId} score=${score.toFixed(
                        4,
                    )}`,
                );
            }
        }
    }

    // 3) Do NOT rewrite ids here. We keep the desired patch's ids so the
    // Renderer/UI stays aligned with the DSL. The Rust engine will consume the
    // Remap hints (moduleIdRemaps) to preserve instances while keeping desired ids.
    const appliedPatch = desiredGraph;

    const moduleIdRemap: Record<string, string> = {};
    for (const [from, to] of idMap.entries()) {
        if (from !== to) {
            moduleIdRemap[from] = to;
        }
    }

    return { appliedPatch, moduleIdRemap };
}
