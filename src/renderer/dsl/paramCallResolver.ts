/**
 * Resolve a DSL call expression to the module schema behind it, and each of
 * its arguments to the param it sets.
 *
 * Handles direct factory calls (`$saw(...)`, `$unstable.filter.lp(...)`),
 * `.$.` / `.$m.` module chains, and the chainable sugar methods
 * (`.amplitude(x)`, `.gain(x)`, ...), whose single argument sets a param of
 * a utility module.
 */
import { Node, SyntaxKind, ts } from 'ts-morph';
import type { CallExpression, Expression, SourceFile } from 'ts-morph';

import type { Schemas } from '../../shared/dsl/schemaTypeResolver';
import type { ParamLiteral, ParamSpec } from './paramControls';

export type SchemaEntry = Schemas[number];

/**
 * Chainable sugar methods on BaseCollection/ModuleOutput in
 * src/main/dsl/GraphBuilder.ts — each wraps its single argument in the named
 * utility module's param, whose schema supplies the control's metadata.
 */
const SUGAR_METHODS: Record<
    string,
    { module: string; param: string; range?: { min: number; max: number } }
> = {
    amp: { module: '$scaleAndShift', param: 'scale' },
    amplitude: { module: '$scaleAndShift', param: 'scale' },
    addHz: { module: '$addHz', param: 'offset' },
    exp: { module: '$curve', param: 'exp' },
    // gain's level is perceptual: 5 = unity, 0 = silence — narrower than
    // scale's schema range.
    gain: {
        module: '$scaleAndShift',
        param: 'scale',
        range: { max: 5, min: 0 },
    },
    mulHz: { module: '$mulHz', param: 'factor' },
    shift: { module: '$scaleAndShift', param: 'shift' },
};

/** The `.$m.` crossfade mix argument: 0 = dry, 5 = wet, 2.5 = equal. */
export const MIX_SPEC: ParamSpec = {
    defaultValue: 2.5,
    maxValue: 5,
    minValue: 0,
    signalType: 'control',
};

export type ResolvedCall =
    | {
          kind: 'direct' | 'chain' | 'chainMix';
          schema: SchemaEntry;
          /** Maps a call-argument index to the schema positionalArgs index,
           *  or null when that call-arg is not a positional module param
           *  (the `.$m.` mix arg). */
          argToPositional: (argIndex: number) => number | null;
          /** Call-argument index where the config object literal sits. */
          configSlot: number;
      }
    | {
          kind: 'sugar';
          /** Method name as written at the call site ('amp', 'gain', ...). */
          name: string;
          spec: ParamSpec;
      };

export function paramSpec(
    schema: SchemaEntry,
    name: string,
): ParamSpec | undefined {
    return schema.signalParams.find((p) => p.name === name);
}

/** A resolver for calls against `schemas`. */
export function createCallResolver(
    schemas: Schemas,
): (call: CallExpression) => ResolvedCall | null {
    const byName = new Map<string, SchemaEntry>();
    const byChainPath = new Map<string, SchemaEntry>();
    const declared = new WeakMap<SourceFile, Set<string>>();
    /**
     * Whether an identifier can hold a DSL signal: a `$` global, or a name
     * the patch itself declares. Rules out JS globals like `Math`, whose
     * methods (`Math.exp`) share names with the sugar methods.
     */
    const isSignalRoot = (id: Node) => {
        const name = id.getText();
        if (name.startsWith('$')) {
            return true;
        }
        const file = id.getSourceFile();
        let names = declared.get(file);
        if (!names) {
            names = declaredNames(file);
            declared.set(file, names);
        }
        return names.has(name);
    };
    for (const s of schemas) {
        byName.set(s.name, s);
        byChainPath.set(s.name.startsWith('$') ? s.name.slice(1) : s.name, s);
    }

    return (call) => {
        const expr = call.getExpression();

        // Direct factory: the callee text is the schema name verbatim
        // ($saw, $unstable.filter.lp).
        const direct = byName.get(expr.getText());
        if (direct) {
            const count = direct.positionalArgs?.length ?? 0;
            return {
                argToPositional: (i) => (i < count ? i : null),
                configSlot: count,
                kind: 'direct',
                schema: direct,
            };
        }

        if (!Node.isPropertyAccessExpression(expr)) {
            return null;
        }

        // Flatten the property chain: `x.$.unstable.filter.lp` yields
        // segments ['$', 'unstable', 'filter', 'lp'] with root `x`.
        const segments: string[] = [];
        let root: Expression = expr;
        while (Node.isPropertyAccessExpression(root)) {
            segments.unshift(root.getName());
            root = root.getExpression();
        }
        if (
            !(Node.isIdentifier(root) && isSignalRoot(root)) &&
            !Node.isCallExpression(root)
        ) {
            return null;
        }

        // `.$.` / `.$m.` chain: the chained signal is injected as the
        // module's first positional argument; `.$m.` adds a leading
        // crossfade mix argument.
        const dollarIdx = Math.max(
            segments.lastIndexOf('$'),
            segments.lastIndexOf('$m'),
        );
        if (dollarIdx >= 0) {
            const schema = byChainPath.get(
                segments.slice(dollarIdx + 1).join('.'),
            );
            if (!schema) {
                return null;
            }
            const count = schema.positionalArgs?.length ?? 0;
            if (segments[dollarIdx] === '$m') {
                return {
                    argToPositional: (i) => (i >= 1 && i < count ? i : null),
                    configSlot: count,
                    kind: 'chainMix',
                    schema,
                };
            }
            return {
                argToPositional: (i) => (i + 1 < count ? i + 1 : null),
                configSlot: count - 1,
                kind: 'chain',
                schema,
            };
        }

        const name = segments[segments.length - 1];
        const sugar = SUGAR_METHODS[name];
        const module = sugar && byName.get(sugar.module);
        const spec = module && paramSpec(module, sugar.param);
        if (!spec) {
            return null;
        }
        return {
            kind: 'sugar',
            name,
            spec: {
                ...spec,
                maxValue: sugar.range?.max ?? spec.maxValue,
                minValue: sugar.range?.min ?? spec.minValue,
            },
        };
    };
}

/** Every name a variable, parameter, or function declaration binds. */
function declaredNames(file: SourceFile): Set<string> {
    const names = new Set<string>();
    for (const kind of [
        SyntaxKind.VariableDeclaration,
        SyntaxKind.Parameter,
        SyntaxKind.BindingElement,
        SyntaxKind.FunctionDeclaration,
    ] as const) {
        for (const decl of file.getDescendantsOfKind(kind)) {
            const name = decl.getNameNode();
            if (name && Node.isIdentifier(name)) {
                names.add(name.getText());
            }
        }
    }
    return names;
}

/** A numeric (optionally negated) or plain string literal, or null for any
 *  other expression. */
export function asLiteral(node: Node): ParamLiteral | null {
    if (Node.isNumericLiteral(node)) {
        return {
            kind: 'number',
            text: node.getText(),
            value: node.getLiteralValue(),
        };
    }
    // Only unary minus: control-literal validation rejects a unary plus.
    if (
        Node.isPrefixUnaryExpression(node) &&
        node.getOperatorToken() === ts.SyntaxKind.MinusToken
    ) {
        const operand = node.getOperand();
        if (Node.isNumericLiteral(operand)) {
            return {
                kind: 'number',
                text: node.getText(),
                value: -operand.getLiteralValue(),
            };
        }
    }
    if (Node.isStringLiteral(node)) {
        return {
            kind: 'string',
            text: node.getText(),
            value: node.getLiteralText(),
        };
    }
    return null;
}
