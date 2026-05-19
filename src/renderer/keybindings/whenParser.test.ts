/**
 * Tests for the when-clause parser. Evaluator takes any object with a
 * `get(key: string): unknown` method, so we use a plain `Map` here.
 */

import { describe, expect, test } from 'vitest';

import { parseWhen, type IContextReader } from './whenParser';

const reader = (entries: Record<string, unknown>): IContextReader => {
    const map = new Map<string, unknown>(Object.entries(entries));
    return { get: (key) => map.get(key) };
};

describe('parseWhen', () => {
    test('empty / null / whitespace inputs evaluate to true', () => {
        expect(parseWhen('').evaluate(reader({}))).toBe(true);
        expect(parseWhen(null).evaluate(reader({}))).toBe(true);
        expect(parseWhen(undefined).evaluate(reader({}))).toBe(true);
        expect(parseWhen('   ').evaluate(reader({}))).toBe(true);
    });

    test('bare identifier evaluates as truthy on the context value', () => {
        const expr = parseWhen('editorFocused');
        expect(expr.evaluate(reader({ editorFocused: true }))).toBe(true);
        expect(expr.evaluate(reader({ editorFocused: false }))).toBe(false);
        expect(expr.evaluate(reader({}))).toBe(false);
        expect(expr.evaluate(reader({ editorFocused: 'yes' }))).toBe(true);
    });

    test('negation flips truthiness', () => {
        expect(parseWhen('!editorFocused').evaluate(reader({}))).toBe(true);
        expect(
            parseWhen('!editorFocused').evaluate(
                reader({ editorFocused: true }),
            ),
        ).toBe(false);
        expect(parseWhen('!!editorFocused').evaluate(reader({}))).toBe(false);
    });

    test('&& short-circuits and respects truthiness of both sides', () => {
        const expr = parseWhen('editorFocused && !suggestWidgetVisible');
        expect(
            expr.evaluate(reader({ editorFocused: true })),
        ).toBe(true);
        expect(
            expr.evaluate(
                reader({ editorFocused: true, suggestWidgetVisible: true }),
            ),
        ).toBe(false);
        expect(expr.evaluate(reader({}))).toBe(false);
    });

    test('|| evaluates either side', () => {
        const expr = parseWhen('editorFocused || fileExplorerFocused');
        expect(
            expr.evaluate(reader({ fileExplorerFocused: true })),
        ).toBe(true);
        expect(expr.evaluate(reader({}))).toBe(false);
    });

    test('&& binds tighter than ||', () => {
        const expr = parseWhen('a || b && c');
        expect(expr.evaluate(reader({ a: true }))).toBe(true);
        expect(expr.evaluate(reader({ b: true, c: true }))).toBe(true);
        expect(expr.evaluate(reader({ b: true }))).toBe(false);
    });

    test('parentheses override precedence', () => {
        const expr = parseWhen('(a || b) && c');
        expect(expr.evaluate(reader({ a: true, c: true }))).toBe(true);
        expect(expr.evaluate(reader({ a: true }))).toBe(false);
    });

    test('equality compares raw values, not truthiness', () => {
        const eq = parseWhen('mode == "edit"');
        expect(eq.evaluate(reader({ mode: 'edit' }))).toBe(true);
        expect(eq.evaluate(reader({ mode: 'view' }))).toBe(false);
        expect(eq.evaluate(reader({}))).toBe(false);

        const ne = parseWhen('mode != "edit"');
        expect(ne.evaluate(reader({ mode: 'edit' }))).toBe(false);
        expect(ne.evaluate(reader({ mode: 'view' }))).toBe(true);
    });

    test('equality against numeric and boolean literals', () => {
        expect(parseWhen('count == 0').evaluate(reader({ count: 0 }))).toBe(
            true,
        );
        expect(
            parseWhen('ready == true').evaluate(reader({ ready: true })),
        ).toBe(true);
        expect(
            parseWhen('ready == false').evaluate(reader({ ready: false })),
        ).toBe(true);
    });

    test('single-quoted strings parse the same as double-quoted', () => {
        const expr = parseWhen("mode == 'edit'");
        expect(expr.evaluate(reader({ mode: 'edit' }))).toBe(true);
    });

    test('dotted identifiers are valid', () => {
        const expr = parseWhen('editor.cursor.atStart');
        expect(
            expr.evaluate(reader({ 'editor.cursor.atStart': true })),
        ).toBe(true);
    });

    test('throws on unterminated string', () => {
        expect(() => parseWhen('mode == "edit')).toThrow();
    });

    test('throws on unexpected character', () => {
        expect(() => parseWhen('a @ b')).toThrow();
    });

    test('throws on trailing garbage', () => {
        expect(() => parseWhen('a b')).toThrow();
    });

    test('throws on missing closing paren', () => {
        expect(() => parseWhen('(a || b')).toThrow();
    });

    test('caches identical sources', () => {
        const a = parseWhen('editorFocused && !suggestWidgetVisible');
        const b = parseWhen('editorFocused && !suggestWidgetVisible');
        expect(a).toBe(b);
    });

    test('preserves source string on the returned expression', () => {
        expect(parseWhen('a && b').source).toContain('a');
        expect(parseWhen('a && b').source).toContain('b');
    });
});
