/**
 * When-clause parser for the context-key service.
 *
 * Grammar (loosely modeled on vscode's, stripped to the ops we use):
 *
 *   expression := orExpr
 *   orExpr     := andExpr ( "||" andExpr )*
 *   andExpr    := unary ( "&&" unary )*
 *   unary      := "!" unary | equality
 *   equality   := primary ( ("==" | "!=") cmpValue )*
 *   cmpValue   := literal | identifier   (a bare identifier here is a string)
 *   primary    := "(" expression ")" | literal | identifier
 *   literal    := true | false | "..." | '...' | number
 *   identifier := [a-zA-Z_] [a-zA-Z0-9_.]*
 *
 * Identifiers reference context keys, except on the right of `==` / `!=`,
 * where a bare word is a string literal (vscode semantics). A bare
 * identifier evaluates as truthy iff its value is truthy. Equality compares
 * the left key's value to the right literal using JS `==` / `!=`.
 */

export interface IContextReader {
    get(key: string): unknown;
}

export interface WhenExpr {
    evaluate(ctx: IContextReader): boolean;
    /** Original source string, retained for debugging / palette display. */
    readonly source: string;
}

const TRUE_EXPR: WhenExpr = {
    evaluate: () => true,
    source: 'true',
};

const FALSE_EXPR: WhenExpr = {
    evaluate: () => false,
    source: 'false',
};

type Token =
    | { kind: 'op'; value: '&&' | '||' | '!' | '==' | '!=' | '(' | ')' }
    | { kind: 'ident'; value: string }
    | { kind: 'string'; value: string }
    | { kind: 'number'; value: number }
    | { kind: 'bool'; value: boolean };

function tokenize(input: string): Token[] {
    const tokens: Token[] = [];
    let i = 0;
    while (i < input.length) {
        const ch = input[i];
        if (ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r') {
            i++;
            continue;
        }
        if (ch === '(' || ch === ')') {
            tokens.push({ kind: 'op', value: ch });
            i++;
            continue;
        }
        if (ch === '&' && input[i + 1] === '&') {
            tokens.push({ kind: 'op', value: '&&' });
            i += 2;
            continue;
        }
        if (ch === '|' && input[i + 1] === '|') {
            tokens.push({ kind: 'op', value: '||' });
            i += 2;
            continue;
        }
        if (ch === '=' && input[i + 1] === '=') {
            tokens.push({ kind: 'op', value: '==' });
            i += 2;
            continue;
        }
        if (ch === '!' && input[i + 1] === '=') {
            tokens.push({ kind: 'op', value: '!=' });
            i += 2;
            continue;
        }
        if (ch === '!') {
            tokens.push({ kind: 'op', value: '!' });
            i++;
            continue;
        }
        if (ch === '"' || ch === "'") {
            const quote = ch;
            let j = i + 1;
            while (j < input.length && input[j] !== quote) j++;
            if (j >= input.length) {
                throw new Error(
                    `[whenParser] unterminated string starting at index ${i}`,
                );
            }
            tokens.push({ kind: 'string', value: input.slice(i + 1, j) });
            i = j + 1;
            continue;
        }
        if (ch >= '0' && ch <= '9') {
            let j = i;
            while (j < input.length && /[0-9.]/.test(input[j])) j++;
            tokens.push({ kind: 'number', value: Number(input.slice(i, j)) });
            i = j;
            continue;
        }
        if (/[a-zA-Z_]/.test(ch)) {
            let j = i;
            while (j < input.length && /[a-zA-Z0-9_.]/.test(input[j])) j++;
            const word = input.slice(i, j);
            if (word === 'true') tokens.push({ kind: 'bool', value: true });
            else if (word === 'false')
                tokens.push({ kind: 'bool', value: false });
            else tokens.push({ kind: 'ident', value: word });
            i = j;
            continue;
        }
        throw new Error(
            `[whenParser] unexpected character "${ch}" at index ${i}`,
        );
    }
    return tokens;
}

/**
 * Internal AST node carrying both a raw-value evaluator (used by `==` /
 * `!=`) and a boolean evaluator (used everywhere else). Equality must
 * compare actual key values, not their truthiness.
 */
interface Node {
    source: string;
    raw(ctx: IContextReader): unknown;
    bool(ctx: IContextReader): boolean;
}

function asWhen(node: Node): WhenExpr {
    return { source: node.source, evaluate: (ctx) => node.bool(ctx) };
}

class Parser {
    private pos = 0;
    constructor(
        private readonly tokens: Token[],
        private readonly source: string,
    ) {}

    parse(): WhenExpr {
        const expr = this.parseOr();
        if (this.pos !== this.tokens.length) {
            throw new Error(
                `[whenParser] unexpected trailing input in "${this.source}" at token ${this.pos}`,
            );
        }
        return asWhen(expr);
    }

    private peek(): Token | undefined {
        return this.tokens[this.pos];
    }

    private consumeOp(
        value: '&&' | '||' | '!' | '==' | '!=' | '(' | ')',
    ): boolean {
        const t = this.peek();
        if (t && t.kind === 'op' && t.value === value) {
            this.pos++;
            return true;
        }
        return false;
    }

    private parseOr(): Node {
        let left = this.parseAnd();
        while (this.consumeOp('||')) {
            const right = this.parseAnd();
            const a = left;
            const b = right;
            const expr: Node = {
                source: `${a.source} || ${b.source}`,
                bool: (ctx) => a.bool(ctx) || b.bool(ctx),
                raw: (ctx) => a.bool(ctx) || b.bool(ctx),
            };
            left = expr;
        }
        return left;
    }

    private parseAnd(): Node {
        let left = this.parseUnary();
        while (this.consumeOp('&&')) {
            const right = this.parseUnary();
            const a = left;
            const b = right;
            const expr: Node = {
                source: `${a.source} && ${b.source}`,
                bool: (ctx) => a.bool(ctx) && b.bool(ctx),
                raw: (ctx) => a.bool(ctx) && b.bool(ctx),
            };
            left = expr;
        }
        return left;
    }

    private parseUnary(): Node {
        if (this.consumeOp('!')) {
            const inner = this.parseUnary();
            return {
                source: `!${inner.source}`,
                bool: (ctx) => !inner.bool(ctx),
                raw: (ctx) => !inner.bool(ctx),
            };
        }
        return this.parseEquality();
    }

    private parseEquality(): Node {
        const left = this.parsePrimary();
        const t = this.peek();
        if (t && t.kind === 'op' && (t.value === '==' || t.value === '!=')) {
            const op = t.value;
            this.pos++;
            const right = this.parseComparisonValue();
            const evalBool = (ctx: IContextReader): boolean => {
                const a = left.raw(ctx);
                const b = right.raw(ctx);
                // eslint-disable-next-line eqeqeq
                return op === '==' ? a == b : a != b;
            };
            return {
                source: `${left.source} ${op} ${right.source}`,
                bool: evalBool,
                raw: evalBool,
            };
        }
        return left;
    }

    /**
     * The right-hand side of `==` / `!=`. A bare identifier here is a string
     * literal — `editorLangId == javascript` compares to the string
     * "javascript" (vscode semantics), not to a context key. Quoted strings,
     * numbers, and booleans fall through to `parsePrimary` as literals.
     */
    private parseComparisonValue(): Node {
        const t = this.peek();
        if (t && t.kind === 'ident') {
            this.pos++;
            const v = t.value;
            return { source: v, bool: () => v.length > 0, raw: () => v };
        }
        return this.parsePrimary();
    }

    private parsePrimary(): Node {
        const t = this.peek();
        if (!t)
            throw new Error(
                `[whenParser] unexpected end of input in "${this.source}"`,
            );

        if (t.kind === 'op' && t.value === '(') {
            this.pos++;
            const inner = this.parseOr();
            if (!this.consumeOp(')')) {
                throw new Error(`[whenParser] missing ")" in "${this.source}"`);
            }
            return {
                source: `(${inner.source})`,
                bool: (ctx) => inner.bool(ctx),
                raw: (ctx) => inner.raw(ctx),
            };
        }
        if (t.kind === 'bool') {
            this.pos++;
            const v = t.value;
            return { source: String(v), bool: () => v, raw: () => v };
        }
        if (t.kind === 'number') {
            this.pos++;
            const v = t.value;
            return { source: String(v), bool: () => Boolean(v), raw: () => v };
        }
        if (t.kind === 'string') {
            this.pos++;
            const v = t.value;
            return { source: `"${v}"`, bool: () => v.length > 0, raw: () => v };
        }
        if (t.kind === 'ident') {
            this.pos++;
            const key = t.value;
            return {
                source: key,
                bool: (ctx) => Boolean(ctx.get(key)),
                raw: (ctx) => ctx.get(key),
            };
        }
        throw new Error(
            `[whenParser] unexpected token ${JSON.stringify(t)} in "${this.source}"`,
        );
    }
}

const parseCache = new Map<string, WhenExpr>();

/**
 * Parse a when-clause string into an expression evaluated against the
 * context-key service. Results are cached per source string.
 *
 * Empty / whitespace-only inputs return an always-true expression so
 * callers can store `when: ""` (or omit it) for unconditional commands.
 */
export function parseWhen(source: string | undefined | null): WhenExpr {
    if (source == null) return TRUE_EXPR;
    const trimmed = source.trim();
    if (trimmed.length === 0) return TRUE_EXPR;
    if (trimmed === 'true') return TRUE_EXPR;
    if (trimmed === 'false') return FALSE_EXPR;
    const cached = parseCache.get(trimmed);
    if (cached) return cached;
    const expr = new Parser(tokenize(trimmed), trimmed).parse();
    parseCache.set(trimmed, expr);
    return expr;
}
