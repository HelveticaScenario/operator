/**
 * Runs the WGSL helper functions of a compiled patch on the CPU, so tests can
 * check what a module computes rather than how its source reads. It covers
 * the subset the helpers use: `let`/`var` bindings, assignments, `return`, scalar
 * and vector arithmetic, swizzles and the common built-ins.
 */

type Value = number | number[];

const SWIZZLE = { r: 0, g: 1, b: 2, a: 3, x: 0, y: 1, z: 2, w: 3 } as const;

const lanes = (v: Value, n: number): number[] =>
    typeof v === 'number' ? Array<number>(n).fill(v) : v;

function zip(a: Value, b: Value, f: (x: number, y: number) => number): Value {
    if (typeof a === 'number' && typeof b === 'number') return f(a, b);
    const n = typeof a === 'number' ? (b as number[]).length : a.length;
    const x = lanes(a, n);
    const y = lanes(b, n);
    return x.map((xi, i) => f(xi, y[i]));
}

function map3(
    a: Value,
    b: Value,
    c: Value,
    f: (x: number, y: number, z: number) => number,
): Value {
    const n = [a, b, c].reduce(
        (m, v) => (typeof v === 'number' ? m : Math.max(m, v.length)),
        0,
    );
    if (n === 0) return f(a as number, b as number, c as number);
    const [x, y, z] = [lanes(a, n), lanes(b, n), lanes(c, n)];
    return x.map((xi, i) => f(xi, y[i], z[i]));
}

const unary = (v: Value, f: (x: number) => number): Value =>
    typeof v === 'number' ? f(v) : v.map(f);

const vector =
    (n: number) =>
    (...args: Value[]): Value => {
        const flat = args.flatMap((a) => (typeof a === 'number' ? [a] : a));
        return flat.length === 1 ? Array<number>(n).fill(flat[0]) : flat;
    };

const smooth = (e0: number, e1: number, x: number) => {
    const t = Math.min(Math.max((x - e0) / (e1 - e0), 0), 1);
    return t * t * (3 - 2 * t);
};

const BUILTINS: Record<string, (...args: Value[]) => Value> = {
    vec2f: vector(2),
    vec3f: vector(3),
    vec4f: vector(4),
    abs: (v) => unary(v, Math.abs),
    floor: (v) => unary(v, Math.floor),
    fract: (v) => unary(v, (x) => x - Math.floor(x)),
    max: (a, b) => zip(a, b, Math.max),
    min: (a, b) => zip(a, b, Math.min),
    step: (e, x) => zip(e, x, (ei, xi) => (xi >= ei ? 1 : 0)),
    clamp: (v, lo, hi) =>
        map3(v, lo, hi, (x, l, h) => Math.min(Math.max(x, l), h)),
    mix: (a, b, t) => map3(a, b, t, (x, y, k) => x + (y - x) * k),
    smoothstep: (e0, e1, x) => map3(e0, e1, x, smooth),
    select: (f, t, cond) => map3(f, t, cond, (a, b, c) => (c ? b : a)),
    dot: (a, b) =>
        (a as number[]).reduce((s, x, i) => s + x * (b as number[])[i], 0),
};

const TOKEN =
    /\s*(\d+\.\d*(?:e-?\d+)?|\d+(?:e-?\d+)?|[A-Za-z_]\w*|[-+*/(),.=;{}:<>])/y;

function tokenize(text: string): string[] {
    const tokens: string[] = [];
    TOKEN.lastIndex = 0;
    while (TOKEN.lastIndex < text.trimEnd().length) {
        const m = TOKEN.exec(text);
        if (m === null)
            throw new Error(`cannot read WGSL at ${TOKEN.lastIndex}`);
        tokens.push(m[1]);
    }
    return tokens;
}

interface Fn {
    params: string[];
    body: string;
}

/** The functions declared in `wgsl`, by name. */
function functions(wgsl: string): Map<string, Fn> {
    const out = new Map<string, Fn>();
    const header = /fn (\w+)\(([^)]*)\)\s*->\s*\w+\s*\{/g;
    let m: RegExpExecArray | null;
    while ((m = header.exec(wgsl)) !== null) {
        let depth = 1;
        let i = header.lastIndex;
        while (depth > 0) {
            if (wgsl[i] === '{') depth++;
            if (wgsl[i] === '}') depth--;
            i++;
        }
        out.set(m[1], {
            params: m[2]
                .split(',')
                .map((p) => p.split(':')[0].trim())
                .filter(Boolean),
            body: wgsl.slice(header.lastIndex, i - 1),
        });
    }
    return out;
}

/** Calls the WGSL function `name` declared in `wgsl` with `args`. */
export function callWgsl(wgsl: string, name: string, ...args: Value[]): Value {
    const fns = functions(wgsl);

    const call = (fname: string, values: Value[]): Value => {
        const builtin = BUILTINS[fname];
        if (builtin !== undefined) return builtin(...values);
        const fn = fns.get(fname);
        if (fn === undefined) throw new Error(`no WGSL function ${fname}`);
        const scope = new Map(fn.params.map((p, i) => [p, values[i]]));
        const t = tokenize(fn.body);
        let pos = 0;
        const peek = () => t[pos];
        const take = (expected?: string) => {
            const tok = t[pos++];
            if (expected !== undefined && tok !== expected) {
                throw new Error(`expected ${expected}, got ${tok}`);
            }
            return tok;
        };

        const primary = (): Value => {
            const tok = take();
            let value: Value;
            if (tok === '(') {
                value = expression();
                take(')');
            } else if (tok === '-') {
                return unary(primary(), (x) => -x);
            } else if (/^\d/.test(tok)) {
                value = Number(tok);
            } else if (peek() === '(') {
                take('(');
                const callArgs: Value[] = [];
                while (peek() !== ')') {
                    callArgs.push(expression());
                    if (peek() === ',') take(',');
                }
                take(')');
                value = call(tok, callArgs);
            } else {
                if (!scope.has(tok)) throw new Error(`unbound ${tok}`);
                value = scope.get(tok)!;
            }
            while (peek() === '.') {
                take('.');
                const picked = take()
                    .split('')
                    .map(
                        (c) =>
                            (value as number[])[
                                SWIZZLE[c as keyof typeof SWIZZLE]
                            ],
                    );
                value = picked.length === 1 ? picked[0] : picked;
            }
            return value;
        };

        const product = (): Value => {
            let value = primary();
            while (peek() === '*' || peek() === '/') {
                const op = take();
                const rhs = primary();
                value = zip(
                    value,
                    rhs,
                    op === '*' ? (a, b) => a * b : (a, b) => a / b,
                );
            }
            return value;
        };

        const sum = (): Value => {
            let value = product();
            while (peek() === '+' || peek() === '-') {
                const op = take();
                const rhs = product();
                value = zip(
                    value,
                    rhs,
                    op === '+' ? (a, b) => a + b : (a, b) => a - b,
                );
            }
            return value;
        };

        /** A sum, or a comparison of two, as 1 for true and 0 for false. */
        const expression = (): Value => {
            const value = sum();
            if (peek() !== '>' && peek() !== '<') return value;
            const op = take() + (t[pos] === '=' ? take() : '');
            const rhs = sum();
            const compare: Record<string, (a: number, b: number) => boolean> = {
                '>': (a, b) => a > b,
                '>=': (a, b) => a >= b,
                '<': (a, b) => a < b,
                '<=': (a, b) => a <= b,
            };
            return zip(value, rhs, (a, b) => (compare[op](a, b) ? 1 : 0));
        };

        while (pos < t.length) {
            const tok = take();
            if (tok === 'return') return expression();
            if (tok === 'let' || tok === 'var') {
                const id = take();
                take('=');
                scope.set(id, expression());
                take(';');
                continue;
            }
            if (!scope.has(tok)) {
                throw new Error(`unsupported WGSL statement at ${tok}`);
            }
            const lane =
                peek() === '.'
                    ? (take('.'), SWIZZLE[take() as keyof typeof SWIZZLE])
                    : undefined;
            const op = peek() === '=' ? '' : take();
            take('=');
            const rhs = expression() as number;
            take(';');
            if (lane === undefined) {
                if (op !== '') throw new Error(`unsupported ${op}= on ${tok}`);
                scope.set(tok, rhs);
            } else {
                const target = [...(scope.get(tok) as number[])];
                target[lane] = op === '+' ? target[lane] + rhs : rhs;
                scope.set(tok, target);
            }
        }
        throw new Error(`${fname} returns nothing`);
    };

    return call(name, args);
}
