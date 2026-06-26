/**
 * Migrate existing `$adsr(...)` patches to keep their pre-0.0.102 envelope.
 *
 * In 0.0.102 `$adsr` got snappier defaults, a new `curve` parameter, and a
 * retrigger input. For a patch written before then, the omitted parameters now
 * resolve to different values, so the envelope sounds different:
 *
 *   - `attack`  default 0.01 → 0.001 s
 *   - `decay`   default 0.1  → 0.05 s
 *   - `release` default 0.1  → 0.01 s
 *   - `sustain` default 5 V, but now drops to 0 V when `decay` is set and
 *     `sustain` is omitted (a plucky AD shape)
 *   - `curve`   new; the old envelope was linear, the new default is 5
 *     (logarithmic attack, exponential decay/release)
 *
 * This pins every call site to the old behavior by filling in the old defaults
 * for any of `attack`, `decay`, `sustain`, `release`, and `curve` the call does
 * not already set. `curve: 0` restores the old linear ramps; because the new
 * retrigger input defaults to the gate, a linear curve also makes a re-attack
 * resume exactly as it did before, so no retrigger parameter is needed.
 * Parameters the patch sets explicitly are left untouched.
 *
 * Three call forms are rewritten:
 *   - Direct:        `$adsr(gate, config?)`        → config slot is arg 1
 *   - Dollar-chain:  `<gate>.$.adsr(config?)`       → config slot is arg 0
 *   - Mix-chain:     `<gate>.$m.adsr(mix, config?)` → config slot is arg 1
 *     (`.$m` injects a leading `mix` crossfade signal, shifting the config)
 *
 * For each, the config (the trailing options object) gains the missing keys:
 *   - no config object yet → append `{ … }` with the full old default set
 *   - an inline config object → insert the missing keys as its first properties
 *
 * Idempotent: a call whose config already sets all five keys is left untouched.
 *
 * Conservatively skipped (reported for manual review, never rewritten):
 *   - other `.adsr(...)` method forms, which can't be told apart from an
 *     unrelated method named `adsr` by syntax alone
 *   - a config passed as a variable or built with a spread, where injecting a
 *     property could be overridden or is not statically safe
 *   - calls with unexpected extra arguments
 *
 * Comments are not rewritten — commented-out code does not affect the running
 * patch.
 */

import { Node, Project, SyntaxKind } from 'ts-morph';
import type { CallExpression, Expression, SourceFile } from 'ts-morph';

import type { Edit } from './migrationEdits';
import { applyEdits } from './migrationEdits';
import type { MigrationMeta } from './migrations/types';

export interface AdsrDefaultsMigrationResult {
    migrated: string;
    callsChanged: number;
    /** Human-readable descriptions of `adsr` calls that look relevant but were
     *  not rewritten automatically, so the user can fix them by hand. */
    skipped: string[];
    error?: string;
}

const ZERO_COUNTS = { callsChanged: 0 } as const;

/** The parameters whose pre-0.0.102 defaults must be pinned, with the old
 *  default each resolved to, in the order they are written. */
const LEGACY_DEFAULTS: ReadonlyArray<readonly [string, string]> = [
    ['attack', '0.01'],
    ['decay', '0.1'],
    ['sustain', '5'],
    ['release', '0.1'],
    ['curve', '0'],
];

/** The number of leading positional arguments before the config object, per
 *  call form, or null when the call is not an `$adsr`-creating call. */
interface AdsrForm {
    /** Count of positional args before the optional config object. */
    positional: number;
}

export function migrateAdsrDefaults(
    source: string,
): AdsrDefaultsMigrationResult {
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

        const form = classifyAdsr(node);
        if (!form) return;

        const args = node.getArguments() as Expression[];

        // Missing a required positional arg — not a shape we can safely touch.
        if (args.length < form.positional) {
            skipped.push(describeCall(node));
            return;
        }

        // No config object yet: add one carrying the full old default set.
        if (args.length === form.positional) {
            const fullConfig = `{ ${LEGACY_DEFAULTS.map(
                ([key, value]) => `${key}: ${value}`,
            ).join(', ')} }`;
            if (args.length > 0) {
                const anchor = args[args.length - 1];
                edits.push({
                    start: anchor.getEnd(),
                    end: anchor.getEnd(),
                    replacement: `, ${fullConfig}`,
                });
            } else {
                // Dollar-chain call with no arguments (`gate.$.adsr()`): insert
                // the config just inside the parentheses.
                const openParen = node.getFirstChildByKind(
                    SyntaxKind.OpenParenToken,
                );
                if (!openParen) {
                    skipped.push(describeCall(node));
                    return;
                }
                edits.push({
                    start: openParen.getEnd(),
                    end: openParen.getEnd(),
                    replacement: fullConfig,
                });
            }
            callsChanged += 1;
            return;
        }

        // Extra args beyond the config slot — unexpected shape.
        if (args.length > form.positional + 1) {
            skipped.push(describeCall(node));
            return;
        }

        const config = args[form.positional];
        if (!Node.isObjectLiteralExpression(config)) {
            // Config passed as a variable / call result — can't inject safely.
            skipped.push(describeCall(node));
            return;
        }

        const props = config.getProperties();
        if (props.some((p) => Node.isSpreadAssignment(p))) {
            // A spread could override an injected value — not statically safe.
            skipped.push(describeCall(node));
            return;
        }

        const missing = LEGACY_DEFAULTS.filter(
            ([key]) => !objectHasKey(props, key),
        );
        if (missing.length === 0) return; // already pinned — idempotent no-op

        const insertion = missing
            .map(([key, value]) => `${key}: ${value}`)
            .join(', ');
        if (props.length === 0) {
            // Empty `{}` (or `{ }`) → emit a clean object with the missing keys.
            edits.push({
                start: config.getStart(),
                end: config.getEnd(),
                replacement: `{ ${insertion} }`,
            });
        } else {
            // Insert before the first property, preserving brace spacing.
            const first = props[0];
            edits.push({
                start: first.getStart(),
                end: first.getStart(),
                replacement: `${insertion}, `,
            });
        }
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
 * Recognize a call that constructs an `$adsr` node and report how many
 * positional arguments precede its config object. Returns null for anything
 * that isn't a rewritable `$adsr` form.
 */
function classifyAdsr(call: CallExpression): AdsrForm | null {
    const expr = call.getExpression();

    // Direct call: `$adsr(gate, config?)`.
    if (Node.isIdentifier(expr)) {
        return expr.getText() === '$adsr' ? { positional: 1 } : null;
    }

    // Method form: `<gate>.$.adsr(...)` / `<gate>.$m.adsr(...)` are the dollar
    // chains (the gate is the receiver). `.$` adds no positional args before
    // the config; `.$m` injects a leading `mix` signal, so the config shifts by
    // one. Any other `.adsr(...)` is ambiguous and left to the skip path.
    if (Node.isPropertyAccessExpression(expr) && expr.getName() === 'adsr') {
        const obj = expr.getExpression();
        if (Node.isPropertyAccessExpression(obj)) {
            if (obj.getName() === '$') return { positional: 0 };
            if (obj.getName() === '$m') return { positional: 1 };
        }
    }
    return null;
}

/**
 * Whether an object literal already keys `name`, across every form
 * `getProperty(name)` misses: shorthand `{ name }`, string-literal
 * `{ 'name': … }`, and computed `{ ['name']: … }`. Catching all of them keeps
 * the migration idempotent and avoids emitting a duplicate key.
 */
function objectHasKey(props: Node[], name: string): boolean {
    return props.some((prop) => {
        if (Node.isShorthandPropertyAssignment(prop)) {
            return prop.getName() === name;
        }
        if (!Node.isPropertyAssignment(prop)) return false;
        const nameNode = prop.getNameNode();
        if (Node.isIdentifier(nameNode)) return nameNode.getText() === name;
        if (Node.isStringLiteral(nameNode)) {
            return nameNode.getLiteralValue() === name;
        }
        if (Node.isComputedPropertyName(nameNode)) {
            const expr = nameNode.getExpression();
            return Node.isStringLiteral(expr) && expr.getLiteralValue() === name;
        }
        return false;
    });
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

/** Registry entry: the new $adsr defaults shipped in v0.0.102. */
export const meta: MigrationMeta = {
    id: 'adsr-legacy-defaults',
    sinceVersion: '0.0.102',
    order: 4,
    title: 'Migrate $adsr to preserve pre-0.0.102 envelope defaults',
    skippedLabel: 'Needs manual review:',
    run(source) {
        const result = migrateAdsrDefaults(source);
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
