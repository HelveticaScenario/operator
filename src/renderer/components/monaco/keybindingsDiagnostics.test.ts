import { describe, expect, test } from 'vitest';
import { validateWhenClauses } from './keybindingsDiagnostics';

describe('validateWhenClauses', () => {
    test('no diagnostics for supported operators', () => {
        const text = `[
  { "key": "ctrl+a", "command": "a", "when": "editorTextFocus && !editorReadonly" },
  { "key": "ctrl+b", "command": "b", "when": "foo == bar || baz != qux" },
  { "key": "ctrl+c", "command": "c", "when": "(a || b) && c" }
]`;
        expect(validateWhenClauses(text)).toEqual([]);
    });

    test('no diagnostics for empty, true, or absent clauses', () => {
        const text = `[
  { "key": "ctrl+a", "command": "a", "when": "" },
  { "key": "ctrl+b", "command": "b", "when": "true" },
  { "key": "ctrl+c", "command": "c" }
]`;
        expect(validateWhenClauses(text)).toEqual([]);
    });

    test('flags the regex-match operator =~ and reports the supported set', () => {
        const text = `[
  { "key": "ctrl+a", "command": "a", "when": "name =~ test" }
]`;
        const diags = validateWhenClauses(text);
        expect(diags).toHaveLength(1);
        expect(diags[0].message).toContain('Supported operators');
        // Offset/length cover the quoted `when` value.
        const slice = text.slice(
            diags[0].offset,
            diags[0].offset + diags[0].length,
        );
        expect(slice).toBe('"name =~ test"');
    });

    test('flags numeric comparison operators', () => {
        const text = `[{ "key": "ctrl+a", "command": "a", "when": "count < 3" }]`;
        expect(validateWhenClauses(text)).toHaveLength(1);
    });

    test('flags the in operator', () => {
        const text = `[{ "key": "ctrl+a", "command": "a", "when": "lang in supported" }]`;
        expect(validateWhenClauses(text)).toHaveLength(1);
    });

    test('reports only the invalid clause among several entries', () => {
        const text = `[
  { "key": "ctrl+a", "command": "a", "when": "editorTextFocus" },
  { "key": "ctrl+b", "command": "b", "when": "x =~ y" },
  { "key": "ctrl+c", "command": "c", "when": "a && b" }
]`;
        const diags = validateWhenClauses(text);
        expect(diags).toHaveLength(1);
        const slice = text.slice(
            diags[0].offset,
            diags[0].offset + diags[0].length,
        );
        expect(slice).toBe('"x =~ y"');
    });

    test('does not throw on malformed / partially-typed JSON', () => {
        expect(() => validateWhenClauses('[{ "when": "a =~ b"')).not.toThrow();
    });
});
