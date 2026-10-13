/**
 * Quick-fix analysis for turning module params into Control panel controls.
 *
 * Given a patch source and a cursor offset, produces the actions the editor
 * can offer there: replacing an existing literal param value with a control,
 * or adding a control for a signal param the call doesn't set yet. Which
 * control, and its label, range, and initial value, come from the param's
 * schema metadata (see paramControls.ts).
 *
 * Pure and renderer-safe: no Monaco or Electron imports, so the analysis is
 * unit-testable against plain source strings.
 */
import { Node, Project, ts } from 'ts-morph';
import type { CallExpression, SourceFile } from 'ts-morph';

import { findControlCalls } from '../../shared/dsl/controlCalls';
import type { Schemas } from '../../shared/dsl/schemaTypeResolver';
import {
    appendObjectArgument,
    insertObjectProperty,
} from './objectPropertyInsert';
import type { CodeStyle } from './objectPropertyInsert';
import {
    asLiteral,
    createCallResolver,
    MIX_SPEC,
    paramSpec,
} from './paramCallResolver';
import type { ResolvedCall } from './paramCallResolver';
import { addChoices, wrapChoices } from './paramControls';
import type { ParamLiteral, ParamSpec } from './paramControls';

export interface ControlQuickFix {
    title: string;
    kind: 'wrap' | 'add';
    /** Half-open [start, end) replacement span in the source; zero-length for pure inserts. */
    span: { start: number; end: number };
    newText: string;
    /** Offset within newText just past the inserted control call. */
    caretOffset: number;
    /** The default choice with the cursor on the replaced literal — surfaces
     *  as isPreferred. */
    preferred?: boolean;
}

export function computeControlQuickFixes(
    source: string,
    cursorOffset: number,
    schemas: Schemas,
    layout: CodeStyle,
): ControlQuickFix[] {
    let sourceFile: SourceFile;
    try {
        const project = new Project({
            compilerOptions: { allowJs: true, checkJs: false, noEmit: true },
            useInMemoryFileSystem: true,
        });
        sourceFile = project.createSourceFile('quickfix.ts', source);
    } catch {
        return [];
    }
    const chainAt = (pos: number): Node[] => {
        const node = pos >= 0 ? sourceFile.getDescendantAtPos(pos) : undefined;
        return node ? [node, ...node.getAncestors()] : [];
    };
    // A caret sits between characters, and getDescendantAtPos returns the
    // token starting at an offset, so a caret just past a literal (`2.5|`)
    // resolves to the following token. Literals are searched from the
    // character left of the caret too, so a caret at either edge is on one.
    const atCursor = chainAt(cursorOffset);
    const literalsAtCursor = [
        ...new Set([...chainAt(cursorOffset - 1), ...atCursor]),
    ];
    if (literalsAtCursor.length === 0) {
        return [];
    }
    // getDescendantAtPos also resolves trivia positions to the following
    // token. Candidates must contain the cursor by their non-trivia range to
    // count as "at the cursor".
    const containsCursor = (n: Node) =>
        cursorOffset >= n.getStart() && cursorOffset <= n.getEnd();

    const resolveCall = createCallResolver(schemas);
    const existingLabels = collectRootLabels(sourceFile);
    const label = (base: string) => dedupeLabel(base, existingLabels);

    /** Fixes replacing the literal `node` as the value of param `name`. */
    const wrapFixes = (
        node: Node,
        literal: ParamLiteral,
        name: string,
        spec: ParamSpec,
    ): ControlQuickFix[] => {
        const onLiteral = containsCursor(node);
        return wrapChoices(name, literal, spec, label, layout.quote).map(
            (choice) => ({
                caretOffset: choice.text.length,
                kind: 'wrap',
                newText: choice.text,
                preferred: onLiteral && choice.preferred,
                span: { end: node.getEnd(), start: node.getStart() },
                title: choice.title,
            }),
        );
    };

    // Replace a literal call argument (positional, chained, sugar, or mix).
    const argWraps = ((): ControlQuickFix[] => {
        for (const node of literalsAtCursor) {
            const literal = asLiteral(node);
            const parent = node.getParent();
            if (
                !literal ||
                !containsCursor(node) ||
                !parent ||
                !Node.isCallExpression(parent)
            ) {
                continue;
            }
            const argIndex = parent.getArguments().indexOf(node);
            const resolved = argIndex >= 0 ? resolveCall(parent) : null;
            if (!resolved) {
                continue;
            }
            if (resolved.kind === 'sugar') {
                if (argIndex === 0) {
                    return wrapFixes(
                        node,
                        literal,
                        resolved.name,
                        resolved.spec,
                    );
                }
                continue;
            }
            if (resolved.kind === 'chainMix' && argIndex === 0) {
                return wrapFixes(node, literal, 'mix', MIX_SPEC);
            }
            const pi = resolved.argToPositional(argIndex);
            const name =
                pi === null
                    ? undefined
                    : resolved.schema.positionalArgs[pi].name;
            const spec = name && paramSpec(resolved.schema, name);
            if (name && spec) {
                return wrapFixes(node, literal, name, spec);
            }
        }
        return [];
    })();

    // Replace a literal in the call's config object.
    const configWraps = ((): ControlQuickFix[] => {
        for (const node of literalsAtCursor) {
            if (!Node.isPropertyAssignment(node) || !containsCursor(node)) {
                continue;
            }
            const obj = node.getParent();
            const call = obj.getParent();
            if (!call || !Node.isCallExpression(call)) {
                continue;
            }
            const resolved = resolveCall(call);
            if (
                !resolved ||
                resolved.kind === 'sugar' ||
                call.getArguments().indexOf(obj) !== resolved.configSlot
            ) {
                continue;
            }
            const key = propertyKeyName(node);
            const spec = key && paramSpec(resolved.schema, key);
            const init = node.getInitializer();
            const literal = init && asLiteral(init);
            if (key && spec && init && literal) {
                return wrapFixes(init, literal, key, spec);
            }
            return [];
        }
        return [];
    })();

    const fixes = argWraps.length > 0 ? argWraps : configWraps;

    // "Add … for <param>" — signal params the innermost resolvable module
    // call doesn't set yet.
    for (const node of atCursor) {
        if (!Node.isCallExpression(node) || !containsCursor(node)) {
            continue;
        }
        const resolved = resolveCall(node);
        if (resolved && resolved.kind !== 'sugar') {
            fixes.push(...addFixes(node, resolved, label, layout));
            break;
        }
    }

    return fixes;
}

/**
 * Build the "add" actions for one resolved module call: every unset signal
 * param that can be inserted without disturbing the call's positional shape.
 */
function addFixes(
    call: CallExpression,
    resolved: Exclude<ResolvedCall, { kind: 'sugar' }>,
    label: (base: string) => string,
    layout: CodeStyle,
): ControlQuickFix[] {
    const { schema } = resolved;
    const positionals = schema.positionalArgs ?? [];
    const args = call.getArguments();

    const supplied = new Set<string>();
    for (let i = 0; i < args.length; i++) {
        const pi = resolved.argToPositional(i);
        if (pi !== null && positionals[pi]) {
            supplied.add(positionals[pi].name);
        }
    }
    // For chains, the injected first positional is always supplied.
    if (resolved.kind !== 'direct' && positionals[0]) {
        supplied.add(positionals[0].name);
    }

    const configArg =
        resolved.configSlot < args.length ? args[resolved.configSlot] : null;
    // A non-object expression in the config slot may set anything.
    if (configArg && !Node.isObjectLiteralExpression(configArg)) {
        return [];
    }
    const configObj = configArg;
    for (const prop of configObj?.getProperties() ?? []) {
        const key = propertyKeyName(prop);
        if (key) {
            supplied.add(key);
        }
    }

    const sourceFile = call.getSourceFile().compilerNode;
    const closeParen = call.getEnd() - 1;
    const fixes: ControlQuickFix[] = [];
    for (const p of schema.signalParams) {
        if (supplied.has(p.name)) {
            continue;
        }
        for (const choice of addChoices(p.name, p, label, layout.quote)) {
            // The control call ends `prop`, so the caret lands at its end.
            const prop = `${p.name}: ${choice.text}`;
            let span: { start: number; end: number };
            let newText: string;
            let caretOffset: number;
            if (configObj || args.length === resolved.configSlot) {
                const edit = configObj
                    ? insertObjectProperty(
                          sourceFile,
                          configObj.compilerNode,
                          prop,
                          layout,
                      )
                    : appendObjectArgument(
                          sourceFile,
                          call.compilerNode,
                          prop,
                          layout,
                      );
                ({ span, newText } = edit);
                caretOffset = edit.propOffset + prop.length;
            } else {
                // Positional slots are still open: only the very next slot
                // can be filled without leaving a gap.
                const nextPi = resolved.argToPositional(args.length);
                if (nextPi === null || positionals[nextPi]?.name !== p.name) {
                    continue;
                }
                span = { end: closeParen, start: closeParen };
                newText = args.length === 0 ? choice.text : `, ${choice.text}`;
                caretOffset = newText.length;
            }
            fixes.push({
                caretOffset,
                kind: 'add',
                newText,
                span,
                title: choice.title,
            });
        }
    }
    return fixes;
}

function propertyKeyName(node: Node): string | null {
    if (Node.isShorthandPropertyAssignment(node)) {
        return node.getName();
    }
    if (!Node.isPropertyAssignment(node)) {
        return null;
    }
    const nameNode = node.getNameNode();
    if (
        Node.isStringLiteral(nameNode) ||
        Node.isNoSubstitutionTemplateLiteral(nameNode)
    ) {
        return nameNode.getLiteralText();
    }
    return nameNode.getText();
}

/**
 * Labels of every ungrouped control in the source. Sliders and buttons share
 * one label namespace per group, and the generated controls are ungrouped.
 */
function collectRootLabels(sourceFile: SourceFile): Set<string> {
    const labels = new Set<string>();
    for (const { kind, call, group } of findControlCalls(
        sourceFile.compilerNode,
    )) {
        const first = call.arguments[0];
        if (
            kind !== '$cGroup' &&
            group.length === 0 &&
            first &&
            ts.isStringLiteral(first)
        ) {
            labels.add(first.text);
        }
    }
    return labels;
}

function dedupeLabel(base: string, taken: Set<string>): string {
    if (!taken.has(base)) {
        return base;
    }
    for (let n = 2; ; n++) {
        const candidate = `${base} ${n}`;
        if (!taken.has(candidate)) {
            return candidate;
        }
    }
}
