/**
 * Argument Span Analyzer
 *
 * Parses DSL source code and extracts absolute character offsets for literal
 * arguments in module factory calls. The registry is keyed by call-site
 * (line:column) for lookup from factory functions at runtime.
 *
 * Additionally builds an interpolation resolution map for template literals
 * containing const variable references. When a template like `${root} e4 g4`
 * interpolates a const string, the resolution map records the const's literal
 * span so that highlights landing inside the interpolation result can be
 * redirected to the original const declaration site. This works recursively
 * for nested template const chains.
 */

import type { CallExpression } from 'ts-morph';
import { Node, VariableDeclarationKind, type SourceFile } from 'ts-morph';
import type { ModuleSchema } from '@modular/core';

import type {
    CallSiteKey,
    InterpolationResolutionMap,
    SourceSpan,
    SpanRegistry,
} from './sourceAnalysisTypes';
import type { ResolvedInterpolation } from '../../shared/dsl/spanTypes';

/**
 * Build a set of factory function names from module schemas.
 * Only includes modules that have positional arguments defined.
 */
function buildFactoryNames(schemas: ModuleSchema[]): Set<string> {
    const names = new Set<string>();

    for (const schema of schemas) {
        // Track all module calls — positional args, config object properties,
        // And const variable references can all contribute trackable spans
        const parts = schema.name.split('.');
        const finalName = parts[parts.length - 1];
        names.add(finalName);

        // Also add the sanitized version (for direct calls like `seqICycle`)
        const sanitized = schema.name.replace(
            /[^a-zA-Z0-9]+(.)?/g,
            (_match, chr: string | undefined) => (chr ? chr.toUpperCase() : ''),
        );
        names.add(sanitized);
    }

    return names;
}

/**
 * Build a map from factory names to their schemas for quick lookup
 */
function buildSchemaMap(schemas: ModuleSchema[]): Map<string, ModuleSchema> {
    const map = new Map<string, ModuleSchema>();

    for (const schema of schemas) {
        const parts = schema.name.split('.');
        const finalName = parts[parts.length - 1];
        map.set(finalName, schema);

        const sanitized = schema.name.replace(
            /[^a-zA-Z0-9]+(.)?/g,
            (_match, chr: string | undefined) => (chr ? chr.toUpperCase() : ''),
        );
        map.set(sanitized, schema);
        map.set(schema.name, schema);
    }

    return map;
}

/**
 * Extract the function name being called from a CallExpression.
 * Handles both simple calls (foo()) and property access calls (seq.iCycle()).
 */
function getCalledFunctionName(call: CallExpression): string | null {
    const expression = call.getExpression();

    // Simple identifier call: foo()
    if (Node.isIdentifier(expression)) {
        return expression.getText();
    }

    // Property access call: obj.method()
    if (Node.isPropertyAccessExpression(expression)) {
        // Return the method name for matching against schema finals
        return expression.getName();
    }

    return null;
}

/**
 * Get the full dotted path for a property access call.
 * e.g., "$.iCycle" for $.iCycle()
 */
function getFullCallPath(call: CallExpression): string | null {
    const expression = call.getExpression();

    if (Node.isIdentifier(expression)) {
        return expression.getText();
    }

    if (Node.isPropertyAccessExpression(expression)) {
        return expression.getText();
    }

    return null;
}

/**
 * Check if a node is a literal that we should track spans for.
 * Includes: string literals, numeric literals, template literals,
 * array literals, and object literals.
 */
function isTrackableLiteral(node: Node): boolean {
    return (
        Node.isStringLiteral(node) ||
        Node.isNumericLiteral(node) ||
        Node.isNoSubstitutionTemplateLiteral(node) ||
        Node.isTemplateExpression(node) ||
        Node.isArrayLiteralExpression(node) ||
        Node.isObjectLiteralExpression(node) ||
        Node.isPrefixUnaryExpression(node) // For negative numbers like -5
    );
}

/**
 * Pre-build a map of const-declared variable names to their literal initializer spans.
 * Only includes variables declared with `const` whose initializer is a trackable literal.
 * Scans top-level statements only (sufficient for flat DSL scripts).
 */
function buildConstLiteralMap(sourceFile: SourceFile): Map<string, SourceSpan> {
    const map = new Map<string, SourceSpan>();

    for (const statement of sourceFile.getStatements()) {
        if (!Node.isVariableStatement(statement)) {
            continue;
        }

        const declList = statement.getDeclarationList();
        if (declList.getDeclarationKind() !== VariableDeclarationKind.Const) {
            continue;
        }

        for (const decl of declList.getDeclarations()) {
            const initializer = decl.getInitializer();
            if (initializer && isTrackableLiteral(initializer)) {
                map.set(decl.getName(), {
                    end: initializer.getEnd(),
                    start: initializer.getStart(),
                });
            }
        }
    }

    return map;
}

/**
 * Pre-build a map of const-declared variable names to their AST initializer nodes.
 * Used for deeper analysis of template expressions within const declarations.
 */
function buildConstNodeMap(sourceFile: SourceFile): Map<string, Node> {
    const map = new Map<string, Node>();

    for (const statement of sourceFile.getStatements()) {
        if (!Node.isVariableStatement(statement)) {
            continue;
        }

        const declList = statement.getDeclarationList();
        if (declList.getDeclarationKind() !== VariableDeclarationKind.Const) {
            continue;
        }

        for (const decl of declList.getDeclarations()) {
            const initializer = decl.getInitializer();
            if (initializer && isTrackableLiteral(initializer)) {
                map.set(decl.getName(), initializer);
            }
        }
    }

    return map;
}

/**
 * Get the string content length of a const literal, stripping quotes.
 * Returns null if the length cannot be statically determined.
 *
 * For simple string literals: `"abc"` → 3
 * For no-substitution template literals: `` `abc` `` → 3
 * For template expressions with all-const interpolations: recursively computed
 */
function getConstStringLength(
    node: Node,
    constNodeMap: Map<string, Node>,
    visited = new Set<string>(),
): number | null {
    if (Node.isStringLiteral(node)) {
        // Strip quotes and compute length
        return node.getLiteralValue().length;
    }

    if (Node.isNoSubstitutionTemplateLiteral(node)) {
        return node.getLiteralValue().length;
    }

    if (Node.isTemplateExpression(node)) {
        // Sum up: head literal + (each span's expression evaluated length + literal part)
        const head = node.getHead();
        // Head text is between ` and ${, strip the backtick
        let totalLength = head.getLiteralText().length;

        for (const span of node.getTemplateSpans()) {
            const expr = span.getExpression();
            const exprLength = getExpressionStringLength(
                expr,
                constNodeMap,
                visited,
            );
            if (exprLength === null) {
                return null;
            } // Can't determine
            totalLength += exprLength;

            // The literal part of the span (between } and next ${ or closing `)
            const literal = span.getLiteral();
            totalLength += literal.getLiteralText().length;
        }

        return totalLength;
    }

    return null;
}

/**
 * Get the evaluated string length of an expression node.
 * Only works for const identifier references with known literal initializers.
 */
function getExpressionStringLength(
    expr: Node,
    constNodeMap: Map<string, Node>,
    visited: Set<string>,
): number | null {
    if (Node.isIdentifier(expr)) {
        const name = expr.getText();
        if (visited.has(name)) {
            return null;
        } // Circular reference
        const constNode = constNodeMap.get(name);
        if (!constNode) {
            return null;
        }

        visited.add(name);
        const result = getConstStringLength(constNode, constNodeMap, visited);
        visited.delete(name);
        return result;
    }

    // Direct literals in interpolation (rarely useful but handle it)
    if (Node.isStringLiteral(expr)) {
        return expr.getLiteralValue().length;
    }
    if (Node.isNumericLiteral(expr)) {
        return expr.getText().length;
    }

    return null;
}

/**
 * Recursively resolve interpolations in a template expression or const reference.
 *
 * For a template like `${root} e4 ${fifth}`:
 * - Computes the evaluated start position and length for each interpolation
 * - If the interpolated const is itself a template with interpolations, recurses
 *
 * @param node - The template expression or const literal node
 * @param constNodeMap - Map of const names to their AST nodes
 * @param constMap - Map of const names to their document spans
 * @returns Array of resolved interpolations, or null if resolution not possible
 */
function resolveInterpolations(
    node: Node,
    constNodeMap: Map<string, Node>,
    constMap: Map<string, SourceSpan>,
): ResolvedInterpolation[] | null {
    if (!Node.isTemplateExpression(node)) {
        return null;
    }

    const resolutions: ResolvedInterpolation[] = [];
    const head = node.getHead();
    let evalOffset = head.getLiteralText().length;

    for (const span of node.getTemplateSpans()) {
        const expr = span.getExpression();

        if (Node.isIdentifier(expr)) {
            const name = expr.getText();
            const constNode = constNodeMap.get(name);
            const constSpan = constMap.get(name);

            if (constNode && constSpan) {
                const evalLength = getConstStringLength(
                    constNode,
                    constNodeMap,
                );

                if (evalLength !== null) {
                    // Recursively resolve nested interpolations within the const
                    const nested = resolveInterpolations(
                        constNode,
                        constNodeMap,
                        constMap,
                    );

                    resolutions.push({
                        constLiteralSpan: constSpan,
                        evaluatedLength: evalLength,
                        evaluatedStart: evalOffset,
                        nestedResolutions: nested ?? undefined,
                    });

                    evalOffset += evalLength;
                } else {
                    // Can't determine length — skip this interpolation but continue
                    // (offset tracking becomes unreliable, bail on remaining interpolations)
                    return resolutions.length > 0 ? resolutions : null;
                }
            } else {
                // Not a const reference — can't resolve. Stop tracking offsets.
                // Any previous resolutions are still valid since their offsets are correct.
                return resolutions.length > 0 ? resolutions : null;
            }
        } else {
            // Non-identifier expression in interpolation — can't resolve
            const exprLength = getExpressionStringLength(
                expr,
                constNodeMap,
                new Set(),
            );
            if (exprLength !== null) {
                evalOffset += exprLength;
            } else {
                return resolutions.length > 0 ? resolutions : null;
            }
        }

        // Add the literal text following this interpolation
        const literal = span.getLiteral();
        evalOffset += literal.getLiteralText().length;
    }

    return resolutions.length > 0 ? resolutions : null;
}

/**
 * Get a trackable span from a node, either directly (if it's a literal)
 * or by resolving a const variable reference to its literal initializer.
 */
function getTrackableSpan(
    node: Node,
    constMap: Map<string, SourceSpan>,
): SourceSpan | null {
    if (isTrackableLiteral(node)) {
        return { end: node.getEnd(), start: node.getStart() };
    }

    // Try resolving const variable reference
    if (Node.isIdentifier(node)) {
        return constMap.get(node.getText()) ?? null;
    }

    return null;
}

/**
 * Get the AST node for a trackable argument, resolving const references.
 * Returns the literal node itself, or the const's initializer node.
 * Used for deeper analysis like interpolation resolution.
 */
function getTrackableNode(
    node: Node,
    constNodeMap: Map<string, Node>,
): Node | null {
    if (isTrackableLiteral(node)) {
        return node;
    }

    if (Node.isIdentifier(node)) {
        return constNodeMap.get(node.getText()) ?? null;
    }

    return null;
}

/**
 * Analyze factory call argument spans in a parsed source file.
 *
 * Walks all call expressions, matches them against module schemas,
 * and records character-offset spans for each trackable argument.
 * Also resolves interpolations in template literals with const references.
 *
 * @param sourceFile - The ts-morph SourceFile to analyze
 * @param schemas - Module schemas to determine which calls to track
 * @param lineOffset - Line offset for wrapper code in new Function()
 * @param firstLineColumnOffset - Column offset for the first line
 * @returns Span registry and interpolation resolution map
 */
export function analyzeArgumentSpans(
    sourceFile: SourceFile,
    schemas: ModuleSchema[],
    lineOffset: number,
    firstLineColumnOffset: number,
): {
    registry: SpanRegistry;
    interpolationResolutions: InterpolationResolutionMap;
} {
    const registry: SpanRegistry = new Map();
    const interpolationResolutions: InterpolationResolutionMap = new Map();
    const factoryNames = buildFactoryNames(schemas);
    const schemaMap = buildSchemaMap(schemas);

    // Pre-build const literal map for resolving variable references
    const constMap = buildConstLiteralMap(sourceFile);
    const constNodeMap = buildConstNodeMap(sourceFile);

    // Walk all call expressions
    sourceFile.forEachDescendant((node: Node) => {
        if (!Node.isCallExpression(node)) {
            return;
        }

        const call = node;
        const funcName = getCalledFunctionName(call);

        // Validate $slider() calls: label (arg 0) and value (arg 1) must be literals
        if (funcName === '$slider') {
            const args = call.getArguments();
            if (args.length >= 1 && !Node.isStringLiteral(args[0])) {
                const { line, column } = sourceFile.getLineAndColumnAtPos(
                    args[0].getStart(),
                );
                throw new Error(
                    `$slider() label (argument 1) must be a string literal at line ${line}, column ${column}`,
                );
            }
            if (
                args.length >= 2 &&
                !Node.isNumericLiteral(args[1]) &&
                !Node.isPrefixUnaryExpression(args[1])
            ) {
                const { line, column } = sourceFile.getLineAndColumnAtPos(
                    args[1].getStart(),
                );
                throw new Error(
                    `$slider() value (argument 2) must be a numeric literal at line ${line}, column ${column}`,
                );
            }
            return; // $slider is not a module factory, skip further processing
        }

        // Track $p(literal) calls: register the literal span under the $p
        // call site so $p() can embed its own argument_span in the returned
        // ParsedPattern at runtime. Factories that receive a ParsedPattern
        // read that span as the argument_span for their pattern param.
        if (funcName === '$p') {
            const pArgs = call.getArguments();
            if (pArgs.length < 1) return;
            const span = getTrackableSpan(pArgs[0], constMap);
            if (!span) return;
            const callStartPos = call.getStart();
            const { line, column } =
                sourceFile.getLineAndColumnAtPos(callStartPos);
            const columnOffset = line === 1 ? firstLineColumnOffset : 0;
            const callKey: CallSiteKey = `${line + lineOffset}:${column + columnOffset}`;
            const argsMap = new Map<string, SourceSpan>();
            argsMap.set('source', span);
            registry.set(callKey, {
                args: argsMap,
                moduleType: '$p',
            });
            // Resolve interpolations for template expressions passed to $p
            const innerNode = getTrackableNode(pArgs[0], constNodeMap);
            if (innerNode) {
                const resolutions = resolveInterpolations(
                    innerNode,
                    constNodeMap,
                    constMap,
                );
                if (resolutions) {
                    const spanKey = `${span.start}:${span.end}`;
                    interpolationResolutions.set(spanKey, resolutions);
                }
            }
            return;
        }

        // Skip if not a tracked factory
        if (!funcName || !factoryNames.has(funcName)) {
            return;
        }

        // Get the schema for this call
        const fullPath = getFullCallPath(call);
        const schema =
            schemaMap.get(funcName) ||
            (fullPath ? schemaMap.get(fullPath) : null);
        if (!schema) {
            return;
        }

        const args = call.getArguments();
        const argsMap = new Map<string, SourceSpan>();

        // Map each positional argument to its arg name
        const positionalArgs = schema.positionalArgs || [];
        for (let i = 0; i < positionalArgs.length && i < args.length; i++) {
            const arg = args[i];
            const argDef = positionalArgs[i];

            // Array literal: register per-element spans as "argName.0", "argName.1", etc.
            if (Node.isArrayLiteralExpression(arg)) {
                const elements = arg.getElements();
                for (let j = 0; j < elements.length; j++) {
                    const elem = elements[j];
                    const elemSpan = getTrackableSpan(elem, constMap);
                    if (elemSpan) {
                        argsMap.set(`${argDef.name}.${j}`, elemSpan);

                        // Resolve interpolations for template expressions
                        const elemNode = getTrackableNode(elem, constNodeMap);
                        if (elemNode) {
                            const resolutions = resolveInterpolations(
                                elemNode,
                                constNodeMap,
                                constMap,
                            );
                            if (resolutions) {
                                const spanKey = `${elemSpan.start}:${elemSpan.end}`;
                                interpolationResolutions.set(
                                    spanKey,
                                    resolutions,
                                );
                            }
                        }
                    }
                }
                continue;
            }

            // Track literals directly, or resolve const variable references
            const span = getTrackableSpan(arg, constMap);
            if (span) {
                argsMap.set(argDef.name, span);

                // Resolve interpolations for template expressions
                const innerNode = getTrackableNode(arg, constNodeMap);
                if (innerNode) {
                    const resolutions = resolveInterpolations(
                        innerNode,
                        constNodeMap,
                        constMap,
                    );
                    if (resolutions) {
                        const spanKey = `${span.start}:${span.end}`;
                        interpolationResolutions.set(spanKey, resolutions);
                    }
                }
            }
        }

        // Check for config object argument (after positional args)
        if (positionalArgs.length < args.length) {
            const configArg = args[positionalArgs.length];

            if (Node.isObjectLiteralExpression(configArg)) {
                for (const prop of configArg.getProperties()) {
                    if (Node.isPropertyAssignment(prop)) {
                        const propName = prop.getName();
                        if (propName === 'id') {
                            continue;
                        }

                        const initializer = prop.getInitializerOrThrow();
                        const span = getTrackableSpan(initializer, constMap);
                        if (span) {
                            argsMap.set(propName, span);

                            // Resolve interpolations for template expressions
                            const innerNode2 = getTrackableNode(
                                initializer,
                                constNodeMap,
                            );
                            if (innerNode2) {
                                const resolutions = resolveInterpolations(
                                    innerNode2,
                                    constNodeMap,
                                    constMap,
                                );
                                if (resolutions) {
                                    const spanKey = `${span.start}:${span.end}`;
                                    interpolationResolutions.set(
                                        spanKey,
                                        resolutions,
                                    );
                                }
                            }
                        }
                    } else if (Node.isShorthandPropertyAssignment(prop)) {
                        const propName = prop.getName();
                        if (propName === 'id') {
                            continue;
                        }

                        // For shorthand { myVar }, resolve the variable to its const literal
                        const span = constMap.get(propName) ?? null;
                        if (span) {
                            argsMap.set(propName, span);

                            // Resolve interpolations if it's a template const
                            const constNode = constNodeMap.get(propName);
                            if (constNode) {
                                const resolutions = resolveInterpolations(
                                    constNode,
                                    constNodeMap,
                                    constMap,
                                );
                                if (resolutions) {
                                    const spanKey = `${span.start}:${span.end}`;
                                    interpolationResolutions.set(
                                        spanKey,
                                        resolutions,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Skip if no trackable arguments
        if (argsMap.size === 0) {
            return;
        }

        // Get the call site position
        // For property access calls like `seq.iCycle()`, V8 stack traces point to the
        // Opening parenthesis, not the start of the expression. So we need to find
        // The position of the `(` in the call.
        const callExpression = call.getExpression();
        let callStartPos: number;

        if (Node.isPropertyAccessExpression(callExpression)) {
            // For `seq.iCycle()`, get the position of `iCycle`
            // The opening paren follows immediately after the method name
            callStartPos = callExpression.getNameNode().getStart();
        } else {
            // For simple calls like `foo()`, use the identifier start
            callStartPos = call.getStart();
        }

        // Both ts-morph and V8 stack traces use 1-based lines and columns.
        // Add the lineOffset to account for wrapper code in new Function().
        const { line, column } = sourceFile.getLineAndColumnAtPos(callStartPos);
        // For line 1 of source, add the firstLineColumnOffset because
        // The function body template indents the first line with spaces.
        const columnOffset = line === 1 ? firstLineColumnOffset : 0;
        const key: CallSiteKey = `${line + lineOffset}:${column + columnOffset}`;

        registry.set(key, {
            args: argsMap,
            moduleType: schema.name,
        });
    });

    return { interpolationResolutions, registry };
}
