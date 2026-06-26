/**
 * Migrate existing `$cheby(...)` patches to keep their pre-DC-blocker behavior.
 *
 * `$cheby` gained a `blockDC` parameter that defaults to `true`: a ~20 Hz
 * high-pass on the output that removes the DC offset its even-order harmonics
 * introduce. Patches written before this default existed assumed no such
 * high-pass (and may rely on `$cheby` as a sub-audio / CV shaper, which the
 * high-pass would strip). This migration pins those call sites to the old
 * behavior by inserting `{ blockDC: false }`.
 *
 * Three call forms are rewritten:
 *   - Direct:       `$cheby(input, amount, config?)`         → config slot is arg 2
 *   - Dollar-chain: `<src>.$.cheby(amount, config?)`          → config slot is arg 1
 *   - Mix-chain:    `<src>.$m.cheby(mix, amount, config?)`    → config slot is arg 2
 *     (`.$m` injects a leading `mix` crossfade signal, shifting the config)
 *
 * For each, the config (the trailing options object) gets `blockDC: false`:
 *   - no config object yet → append `{ blockDC: false }` as a new argument
 *   - an inline config object → insert `blockDC: false` as its first property
 *
 * Idempotent: a call whose config already mentions `blockDC` is left untouched.
 *
 * Conservatively skipped (reported for manual review, never rewritten):
 *   - other `.cheby(...)` method forms (e.g. `pipeMix`), which can't be told
 *     apart from an unrelated method named `cheby` by syntax alone
 *   - a config passed as a variable or built with a spread, where injecting a
 *     property could be overridden or is not statically safe
 *   - calls missing their required positional arguments
 *
 * Comments are not rewritten — commented-out code does not affect the running
 * patch.
 */

import { Node, Project } from 'ts-morph';
import type {
    CallExpression,
    Expression,
    ObjectLiteralExpression,
    SourceFile,
} from 'ts-morph';

import type { Edit } from './migrationEdits';
import { applyEdits } from './migrationEdits';
import type { MigrationMeta } from './migrations/types';

export interface ChebyBlockDcMigrationResult {
    migrated: string;
    callsChanged: number;
    /** Human-readable descriptions of `cheby` calls that look relevant but were
     *  not rewritten automatically, so the user can fix them by hand. */
    skipped: string[];
    error?: string;
}

const ZERO_COUNTS = { callsChanged: 0 } as const;

/** The number of leading positional arguments before the config object, per
 *  call form, or null when the call is not a `$cheby`-creating call. */
interface ChebyForm {
    /** Count of positional args before the optional config object. */
    positional: number;
}

export function migrateChebyBlockDC(
    source: string,
): ChebyBlockDcMigrationResult {
    let sourceFile: SourceFile;
    try {
        const project = new Project({
            compilerOptions: { allowJs: true, checkJs: false, noEmit: true },
            useInMemoryFileSystem: true,
        });
        sourceFile = project.createSourceFile('migrate.ts', source);
    } catch (err) {
        return {
            migrated: source,
            ...ZERO_COUNTS,
            skipped: [],
            error: err instanceof Error ? err.message : String(err),
        };
    }

    const edits: Edit[] = [];
    const skipped: string[] = [];
    let callsChanged = 0;

    sourceFile.forEachDescendant((node) => {
        if (!Node.isCallExpression(node)) return;

        const form = classifyCheby(node);
        if (!form) return;

        const args = node.getArguments() as Expression[];

        // Missing a required positional arg — not a shape we can safely touch.
        if (args.length < form.positional) {
            skipped.push(describeCall(node));
            return;
        }

        // No config object yet: append one after the last positional arg.
        if (args.length === form.positional) {
            const anchor = args[args.length - 1];
            edits.push({
                start: anchor.getEnd(),
                end: anchor.getEnd(),
                replacement: ', { blockDC: false }',
            });
            callsChanged += 1;
            return;
        }

        // A config argument exists in the config slot.
        const config = args[form.positional];

        // Extra trailing args beyond the config slot — unexpected shape.
        if (args.length > form.positional + 1) {
            skipped.push(describeCall(node));
            return;
        }

        if (!Node.isObjectLiteralExpression(config)) {
            // Config passed as a variable / call result — can't inject safely.
            skipped.push(describeCall(node));
            return;
        }

        const edit = injectBlockDc(config);
        if (edit === 'present') return; // already set — idempotent no-op
        if (edit === 'unsafe') {
            skipped.push(describeCall(node));
            return;
        }
        edits.push(edit);
        callsChanged += 1;
    });

    const { source: migrated, conflict } = applyEdits(source, edits);
    if (conflict) {
        return {
            migrated: source,
            ...ZERO_COUNTS,
            skipped: [],
            error: 'edits conflict',
        };
    }

    return { migrated, callsChanged, skipped: dedupe(skipped) };
}

/**
 * Recognize a call that constructs a `$cheby` node and report how many
 * positional arguments precede its config object. Returns null for anything
 * that isn't a rewritable `$cheby` form.
 */
function classifyCheby(call: CallExpression): ChebyForm | null {
    const expr = call.getExpression();

    // Direct call: `$cheby(input, amount, config?)`.
    if (Node.isIdentifier(expr)) {
        return expr.getText() === '$cheby' ? { positional: 2 } : null;
    }

    // Method form: `<recv>.$.cheby(...)` / `<recv>.$m.cheby(...)` are the dollar
    // chains (input is the receiver). `.$` leaves `amount` as the only
    // positional; `.$m` injects a leading `mix` signal, so `amount` is preceded
    // by it. Any other `.cheby(...)` is ambiguous and left to the skip path.
    if (Node.isPropertyAccessExpression(expr) && expr.getName() === 'cheby') {
        const obj = expr.getExpression();
        if (Node.isPropertyAccessExpression(obj)) {
            if (obj.getName() === '$') return { positional: 1 };
            if (obj.getName() === '$m') return { positional: 2 };
        }
    }
    return null;
}

/**
 * Produce the edit that inserts `blockDC: false` as the first property of an
 * inline config object. Returns `'present'` if `blockDC` is already set (no-op),
 * or `'unsafe'` if the object uses a spread (where a later spread could
 * override the injected value).
 */
function injectBlockDc(
    config: ObjectLiteralExpression,
): Edit | 'present' | 'unsafe' {
    const props = config.getProperties();
    if (props.some(isBlockDcKey)) return 'present';
    if (props.some((p) => Node.isSpreadAssignment(p))) return 'unsafe';

    if (props.length === 0) {
        // Empty `{}` (or `{ }`) → emit a clean `{ blockDC: false }`.
        return {
            start: config.getStart(),
            end: config.getEnd(),
            replacement: '{ blockDC: false }',
        };
    }

    // Insert before the first property, preserving the object's brace spacing.
    const first = props[0];
    return {
        start: first.getStart(),
        end: first.getStart(),
        replacement: 'blockDC: false, ',
    };
}

/**
 * Whether an object-literal member already keys `blockDC`, across every key
 * form `getProperty('blockDC')` misses: shorthand `{ blockDC }`, string-literal
 * `{ 'blockDC': … }`, and computed `{ ['blockDC']: … }`. Catching all of them
 * keeps the migration idempotent and avoids emitting a duplicate key.
 */
function isBlockDcKey(prop: Node): boolean {
    if (Node.isShorthandPropertyAssignment(prop)) {
        return prop.getName() === 'blockDC';
    }
    if (!Node.isPropertyAssignment(prop)) return false;
    const name = prop.getNameNode();
    if (Node.isIdentifier(name)) return name.getText() === 'blockDC';
    if (Node.isStringLiteral(name)) return name.getLiteralValue() === 'blockDC';
    if (Node.isComputedPropertyName(name)) {
        const expr = name.getExpression();
        return (
            Node.isStringLiteral(expr) && expr.getLiteralValue() === 'blockDC'
        );
    }
    return false;
}

function describeCall(call: CallExpression): string {
    const line = call.getStartLineNumber();
    const text = call.getText().replace(/\s+/g, ' ');
    const short = text.length > 60 ? `${text.slice(0, 57)}…` : text;
    return `line ${line}: ${short}`;
}

function dedupe(items: string[]): string[] {
    return Array.from(new Set(items));
}

/** Registry entry: the DC-blocker default shipped in v0.0.102. */
export const meta: MigrationMeta = {
    id: 'cheby-block-dc',
    sinceVersion: '0.0.102',
    order: 3,
    title: 'Migrate $cheby to preserve pre-DC-blocker output',
    skippedLabel: 'Needs manual review:',
    run(source) {
        const result = migrateChebyBlockDC(source);
        return {
            migrated: result.migrated,
            changed: result.migrated !== source,
            summary: {
                callsChanged: result.callsChanged,
                commentsChanged: 0,
                skippedVariables: result.skipped,
                error: result.error,
            },
        };
    },
};
