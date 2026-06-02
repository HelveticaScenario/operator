/**
 * Thin wrapper around the peggy-generated mini-notation parser.
 *
 * Exposes a stable API for parsing a mini-notation source string into a
 * `MiniAST`, converting parser exceptions into `MiniParseError` so callers
 * can present editor-friendly error messages.
 */

// The generated file is `@ts-nocheck`, so TypeScript does not see the
// full signature. We type the imported parse function manually here to
// preserve editor-friendly errors on the wrapper boundary.
import { parse as peggyParse_ } from './grammar.generated';
import type { MiniAST } from './ast';

const peggyParse = peggyParse_ as unknown as (source: string) => MiniAST;

/** Error thrown when a mini-notation source string fails to parse. */
export class MiniParseError extends Error {
    /** 0-indexed start offset where parsing failed, if available. */
    readonly start?: number;
    /** 0-indexed end offset where parsing failed, if available. */
    readonly end?: number;

    constructor(message: string, start?: number, end?: number) {
        super(message);
        this.name = 'MiniParseError';
        this.start = start;
        this.end = end;
    }
}

/**
 * Parse a mini-notation source string into a `MiniAST`.
 *
 * Throws `MiniParseError` on failure. The returned AST is structurally
 * identical to what the Rust `MiniAST` deserializer expects.
 */
export function parseMini(source: string): MiniAST {
    try {
        return peggyParse(source);
    } catch (err: unknown) {
        if (err && typeof err === 'object') {
            const e = err as {
                message?: string;
                location?: {
                    start?: { offset?: number };
                    end?: { offset?: number };
                };
            };
            const start = e.location?.start?.offset;
            const end = e.location?.end?.offset;
            throw new MiniParseError(
                e.message ?? 'mini-notation parse error',
                start,
                end,
            );
        }
        throw new MiniParseError(String(err));
    }
}
