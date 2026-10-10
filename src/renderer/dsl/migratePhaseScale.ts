/**
 * Migrate patches written for 0 to 1 phase signals to the 0 to 5V phase scale.
 *
 * Phases are volts, one cycle per 5V: `$ramp` outputs 0 to 5V, the phase
 * oscillators and phase-distortion modules read 0 to 5V, `phaseOffset` is
 * 0 to 5V, and `$table` warp parameters span 0 to 5V (bend -5 to 5V). Literal
 * values written for the old scale are multiplied by 5:
 *   - `phaseOffset: 0.25` on `$sine`, `$saw`, `$pulse` and `$wavetable`
 *   - a literal phase input of `$pSine`, `$pSaw`, `$pPulse`, `$crush`,
 *     `$feedback` and `$pulsar`
 *   - the first argument of `$table.mirror`, `.bend`, `.sync`, `.fold`, `.pwm`
 *
 * A value is rescaled when it is a number, a `$slider(label, value, min, max)`
 * with numeric arguments, a `$g1`...`$gN` group of such values, or an array of
 * them. Anything else (a variable, a signal) is reported for manual review.
 * Every `$ramp(...)` whose output is read raw, rather than through `.range`,
 * a `.$` chain or a phase module, is reported too, as it now spans 0 to 5V.
 *
 * Not idempotent: running it again multiplies by 5 again, so review the diff
 * and apply it once. Comments are not rewritten.
 */

import { Node, Project, SyntaxKind } from 'ts-morph';
import type { CallExpression, Expression, SourceFile } from 'ts-morph';

import type { Edit } from './migrationEdits';
import { applyEdits } from './migrationEdits';

export interface PhaseScaleMigrationResult {
    migrated: string;
    callsChanged: number;
    /** Human-readable descriptions of values that need a manual look. */
    skipped: string[];
    error?: string;
}

const PHASE_SCALE = 5;
const OFFSET_MODULES = new Set(['$sine', '$saw', '$pulse', '$wavetable']);
const PHASE_INPUT_MODULES = new Set([
    '$pSine',
    '$pSaw',
    '$pPulse',
    '$crush',
    '$feedback',
    '$pulsar',
]);
const TABLE_WARPS = new Set(['mirror', 'bend', 'sync', 'fold', 'pwm']);

function scaled(value: number): string {
    return String(Number.parseFloat((value * PHASE_SCALE).toPrecision(12)));
}

/** The module name a call constructs: `$sine(...)` or `x.$.sine(...)` as `$sine`. */
function moduleName(call: CallExpression): string | null {
    const expr = call.getExpression();
    if (Node.isIdentifier(expr)) return expr.getText();
    if (Node.isPropertyAccessExpression(expr)) {
        const receiver = expr.getExpression();
        if (
            Node.isPropertyAccessExpression(receiver) &&
            receiver.getName() === '$'
        ) {
            return `$${expr.getName()}`;
        }
    }
    return null;
}

/** The number a literal denotes, including a negated one, or null. */
function literalNumber(node: Node): number | null {
    if (Node.isNumericLiteral(node)) return node.getLiteralValue();
    if (
        Node.isPrefixUnaryExpression(node) &&
        node.getOperatorToken() === SyntaxKind.MinusToken
    ) {
        const operand = node.getOperand();
        if (Node.isNumericLiteral(operand)) return -operand.getLiteralValue();
    }
    return null;
}

/** Edits that rescale `node`, or null when it is not statically rescalable. */
function scaleEdits(node: Node): Edit[] | null {
    const number = literalNumber(node);
    if (number !== null) {
        return [
            {
                start: node.getStart(),
                end: node.getEnd(),
                replacement: scaled(number),
            },
        ];
    }
    if (Node.isArrayLiteralExpression(node)) {
        return scaleAll(node.getElements());
    }
    if (Node.isCallExpression(node)) {
        const callee = node.getExpression();
        const name = Node.isIdentifier(callee) ? callee.getText() : '';
        const args = node.getArguments();
        if (/^\$g\d+$/.test(name)) return scaleAll(args);
        if (name === '$slider') return scaleAll(args.slice(1, 4));
    }
    return null;
}

function scaleAll(nodes: Node[]): Edit[] | null {
    const edits: Edit[] = [];
    for (const node of nodes) {
        const each = scaleEdits(node);
        if (each === null) return null;
        edits.push(...each);
    }
    return edits;
}

export function migratePhaseScale(source: string): PhaseScaleMigrationResult {
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
            callsChanged: 0,
            skipped: [],
            error: err instanceof Error ? err.message : String(err),
        };
    }

    const edits: Edit[] = [];
    const skipped: string[] = [];
    let callsChanged = 0;

    const rescale = (value: Node, report: Node, literalOnly = false): void => {
        const found = scaleEdits(value);
        if (found === null) {
            if (!literalOnly) skipped.push(describe(report));
            return;
        }
        edits.push(...found);
        callsChanged += 1;
    };

    sourceFile.forEachDescendant((node) => {
        if (
            Node.isPropertyAssignment(node) &&
            node.getName() === 'phaseOffset'
        ) {
            const call = node.getParent()?.getParent();
            if (
                call &&
                Node.isCallExpression(call) &&
                OFFSET_MODULES.has(moduleName(call) ?? '')
            ) {
                const value = node.getInitializer();
                if (value && !isRampRead(value)) rescale(value, node);
            }
            return;
        }

        if (!Node.isCallExpression(node)) return;

        const name = moduleName(node);
        const callee = node.getExpression();
        const args = node.getArguments() as Expression[];

        if (
            Node.isIdentifier(callee) &&
            PHASE_INPUT_MODULES.has(name ?? '') &&
            args.length > 0
        ) {
            rescale(args[0], node, true);
            return;
        }

        if (
            Node.isPropertyAccessExpression(callee) &&
            TABLE_WARPS.has(callee.getName()) &&
            callee.getExpression().getText() === '$table' &&
            args.length > 0
        ) {
            rescale(args[0], node);
            return;
        }

        if (name === '$ramp' && Node.isIdentifier(callee) && isRawRead(node)) {
            skipped.push(
                `${describe(node)} (now outputs 0 to 5V; check what reads it)`,
            );
        }
    });

    const { source: migrated, conflict } = applyEdits(source, edits);
    if (conflict) {
        return {
            migrated: source,
            callsChanged: 0,
            skipped: [],
            error: 'edits conflict',
        };
    }
    return { migrated, callsChanged, skipped: Array.from(new Set(skipped)) };
}

/** Whether a `$ramp(...)` is the value itself, already on the phase scale. */
function isRampRead(node: Node): boolean {
    return (
        Node.isCallExpression(node) &&
        Node.isIdentifier(node.getExpression()) &&
        node.getExpression().getText() === '$ramp'
    );
}

/** Whether a `$ramp(...)` output is consumed in a way the new scale can break. */
function isRawRead(ramp: CallExpression): boolean {
    const parent = ramp.getParent();
    if (parent && Node.isPropertyAccessExpression(parent)) {
        return !['range', '$'].includes(parent.getName());
    }
    if (parent && Node.isCallExpression(parent)) {
        const inputs = parent.getArguments();
        const consumer = moduleName(parent) ?? '';
        const isPhaseSink =
            PHASE_INPUT_MODULES.has(consumer) && inputs[0] === ramp;
        return !isPhaseSink;
    }
    if (parent && Node.isPropertyAssignment(parent)) {
        return parent.getName() !== 'phaseOffset';
    }
    return true;
}

function describe(node: Node): string {
    const line = node.getStartLineNumber();
    const text = node.getText().replace(/\s+/g, ' ');
    const short = text.length > 60 ? `${text.slice(0, 57)}…` : text;
    return `line ${line}: ${short}`;
}
