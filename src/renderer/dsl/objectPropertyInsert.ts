/**
 * Source edits that add a property to an object literal laid out the way
 * Prettier would print it, without reformatting anything else in the file.
 *
 * Prettier keeps an object expanded when the original has a newline between
 * `{` and its first property; otherwise it prints the object on one line
 * when that fits `printWidth`, and one property per line when it doesn't.
 */
import type { ts } from 'ts-morph';

import type { PrettierConfig } from '../../shared/ipcTypes';

/** The Prettier options generated code follows. */
export interface CodeStyle {
    printWidth: number;
    tabWidth: number;
    useTabs: boolean;
    bracketSpacing: boolean;
    /** Whether expanded objects end with a trailing comma. */
    trailingComma: boolean;
    /** Delimiter for generated string literals. */
    quote: "'" | '"';
}

/** Unset options fall back to Prettier's own defaults. */
export function codeStyleFromPrettier(config: PrettierConfig): CodeStyle {
    return {
        bracketSpacing: config.bracketSpacing !== false,
        printWidth: config.printWidth ?? 80,
        quote: config.singleQuote === true ? "'" : '"',
        tabWidth: config.tabWidth ?? 2,
        trailingComma: (config.trailingComma ?? 'all') !== 'none',
        useTabs: config.useTabs === true,
    };
}

export interface PropertyEdit {
    /** Half-open [start, end) replacement span in the source. */
    span: { start: number; end: number };
    newText: string;
    /** Offset within newText where the inserted property text begins. */
    propOffset: number;
}

/** Add `prop` (e.g. `fm: 1`) as the last property of `obj`. */
export function insertObjectProperty(
    sourceFile: ts.SourceFile,
    obj: ts.ObjectLiteralExpression,
    prop: string,
    layout: CodeStyle,
): PropertyEdit {
    const source = sourceFile.text;
    const open = obj.getStart(sourceFile);
    const close = obj.getEnd() - 1;
    const props = obj.properties;

    if (props.length === 0) {
        const inner = source.slice(open + 1, close);
        // Keep a comment-bearing body intact; only whitespace is replaced.
        const span =
            inner.trim() === ''
                ? { end: close, start: open + 1 }
                : { end: close, start: close };
        const pad = layout.bracketSpacing ? ' ' : '';
        const edit = { newText: `${pad}${prop}${pad}`, propOffset: pad.length, span };
        return fitsOnLine(source, edit, layout)
            ? edit
            : expandObject(sourceFile, obj, prop, layout) ?? edit;
    }

    const last = props[props.length - 1];
    // The last property's trailing comma, possibly after a comment.
    const comma = /^(?:\s|\/\*[\s\S]*?\*\/|\/\/[^\n]*)*,/.exec(
        source.slice(last.getEnd(), close),
    );
    const afterLast = comma ? last.getEnd() + comma[0].length : last.getEnd();
    const nl = eolOf(source);

    const expanded = source
        .slice(open + 1, props[0].getStart(sourceFile))
        .includes('\n');
    if (expanded) {
        // Insert at the end of the last property's line so a trailing line
        // comment stays attached to the property it annotates.
        const eol = lineEnd(source, afterLast);
        const insertAt = eol < close ? eol : afterLast;
        const indent = indentAt(source, last.getStart(sourceFile));
        if (comma) {
            return {
                newText: `${nl}${indent}${prop},`,
                propOffset: nl.length + indent.length,
                span: { end: insertAt, start: insertAt },
            };
        }
        const tail = source.slice(last.getEnd(), insertAt);
        const newText = `,${tail}${nl}${indent}${prop}`;
        return {
            newText,
            propOffset: newText.length - prop.length,
            span: { end: insertAt, start: last.getEnd() },
        };
    }

    const edit = comma
        ? {
              newText: ` ${prop}`,
              propOffset: 1,
              span: { end: afterLast, start: afterLast },
          }
        : {
              newText: `, ${prop}`,
              propOffset: 2,
              span: { end: last.getEnd(), start: last.getEnd() },
          };
    return fitsOnLine(source, edit, layout)
        ? edit
        : expandObject(sourceFile, obj, prop, layout) ?? edit;
}

/**
 * Append a new trailing object argument `{ prop }` to `call`. Too wide for
 * one line, the object breaks the way Prettier hugs a last-argument object:
 * `f(a, {⏎  prop,⏎})`.
 */
export function appendObjectArgument(
    sourceFile: ts.SourceFile,
    call: ts.CallExpression,
    prop: string,
    layout: CodeStyle,
): PropertyEdit {
    const source = sourceFile.text;
    const closeParen = call.getEnd() - 1;
    const args = call.arguments;
    // After a trailing comma the separator is already in place.
    const lead =
        args.length === 0 ? '' : args.hasTrailingComma ? ' ' : ', ';
    const pad = layout.bracketSpacing ? ' ' : '';
    const span = { end: closeParen, start: closeParen };
    const inline = {
        newText: `${lead}{${pad}${prop}${pad}}`,
        propOffset: lead.length + 1 + pad.length,
        span,
    };
    if (fitsOnLine(source, inline, layout)) {
        return inline;
    }
    const nl = eolOf(source);
    const outer = indentAt(source, closeParen);
    const inner = outer + indentUnit(layout);
    const head = `${lead}{${nl}${inner}`;
    return {
        newText: `${head}${prop}${layout.trailingComma ? ',' : ''}${nl}${outer}}`,
        propOffset: head.length,
        span,
    };
}

/**
 * Rewrite `obj` one property per line with `prop` appended, or null when
 * the object holds comments a rebuild from property text would drop.
 */
function expandObject(
    sourceFile: ts.SourceFile,
    obj: ts.ObjectLiteralExpression,
    prop: string,
    layout: CodeStyle,
): PropertyEdit | null {
    const source = sourceFile.text;
    const props = obj.properties;
    let residual = obj.getText(sourceFile);
    for (const p of props) {
        residual = residual.replace(p.getText(sourceFile), '');
    }
    if (/\/[/*]/.test(residual)) {
        return null;
    }

    const nl = eolOf(source);
    const outer = indentAt(source, obj.getStart(sourceFile));
    const inner = outer + indentUnit(layout);
    const existing = props
        .map((p) => `${inner}${p.getText(sourceFile)},${nl}`)
        .join('');
    const head = `{${nl}${existing}${inner}`;
    return {
        newText: `${head}${prop}${layout.trailingComma ? ',' : ''}${nl}${outer}}`,
        propOffset: head.length,
        span: { end: obj.getEnd(), start: obj.getStart(sourceFile) },
    };
}

/** Whether the line holding a single-line edit stays within printWidth. */
function fitsOnLine(
    source: string,
    edit: PropertyEdit,
    layout: CodeStyle,
): boolean {
    const line =
        source.slice(lineStart(source, edit.span.start), edit.span.start) +
        edit.newText +
        source.slice(edit.span.end, lineEnd(source, edit.span.end));
    return visualWidth(line, layout.tabWidth) <= layout.printWidth;
}

function indentUnit(layout: CodeStyle): string {
    return layout.useTabs ? '\t' : ' '.repeat(layout.tabWidth);
}

function lineStart(source: string, pos: number): number {
    return source.lastIndexOf('\n', pos - 1) + 1;
}

/** Offset of the line break ending the line at `pos` (the `\r` of a
 *  `\r\n`), or the source length on the last line. */
function lineEnd(source: string, pos: number): number {
    const end = source.indexOf('\n', pos);
    if (end === -1) {
        return source.length;
    }
    return end > pos && source[end - 1] === '\r' ? end - 1 : end;
}

/**
 * The source's line break. Inserted text uses it verbatim so an editor
 * model normalizes nothing and offsets into the inserted text stay exact.
 */
function eolOf(source: string): string {
    return source.includes('\r\n') ? '\r\n' : '\n';
}

/** Leading whitespace of the line containing `pos`. */
function indentAt(source: string, pos: number): string {
    const start = lineStart(source, pos);
    return /^[ \t]*/.exec(source.slice(start, lineEnd(source, start)))![0];
}

function visualWidth(text: string, tabWidth: number): number {
    let width = 0;
    for (const ch of text) {
        width += ch === '\t' ? tabWidth : 1;
    }
    return width;
}
