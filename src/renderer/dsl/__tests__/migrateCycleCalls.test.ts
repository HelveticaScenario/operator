import { describe, expect, test } from 'vitest';

import { migrateCycleCalls } from '../migrateCycleCalls';

describe('migrateCycleCalls', () => {
    test('wraps a bare string literal', () => {
        const result = migrateCycleCalls(`$cycle("c4 e4 g4");`);
        expect(result.migrated).toBe(`$cycle($p("c4 e4 g4"));`);
        expect(result.callsChanged).toBe(1);
        expect(result.assignmentsChanged).toBe(0);
        expect(result.commentsChanged).toBe(0);
        expect(result.skippedVariables).toEqual([]);
    });

    test('idempotent — already $p()-wrapped input unchanged', () => {
        const source = `$cycle($p("c4 e4"));`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
    });

    test('strips voltage suffix while wrapping', () => {
        const result = migrateCycleCalls(`$cycle("5v 3v");`);
        expect(result.migrated).toBe(`$cycle($p("5 3"));`);
        expect(result.callsChanged).toBe(1);
    });

    test('strips decimal and negative voltage atoms', () => {
        const result = migrateCycleCalls(`$cycle("0.5v -3v 12v");`);
        expect(result.migrated).toBe(`$cycle($p("0.5 -3 12"));`);
        expect(result.callsChanged).toBe(1);
    });

    test('voltage suffix is case-insensitive', () => {
        const result = migrateCycleCalls(`$cycle("5V 2v");`);
        expect(result.migrated).toBe(`$cycle($p("5 2"));`);
    });

    test('leaves note octaves and identifiers intact', () => {
        // c5 is a note (untouched); 5v is a voltage atom (stripped);
        // 5val is an identifier-ish token (untouched).
        const result = migrateCycleCalls(`$cycle("c5 5v 5val");`);
        expect(result.migrated).toBe(`$cycle($p("c5 5 5val"));`);
    });

    test('strips voltage in traced variable assignment', () => {
        const source = `const pat = "5v 7v";\n$cycle(pat);`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `const pat = $p("5 7");\n$cycle(pat);`,
        );
        expect(result.assignmentsChanged).toBe(1);
    });

    test('strips voltage in comment $cycle', () => {
        const result = migrateCycleCalls(`// old: $cycle("5v")`);
        expect(result.migrated).toBe(`// old: $cycle($p("5"))`);
        expect(result.commentsChanged).toBe(1);
    });

    test('strips voltage inside template literal', () => {
        const result = migrateCycleCalls('$cycle(`${root} 5v`);');
        expect(result.migrated).toBe('$cycle($p(`${root} 5`));');
        expect(result.callsChanged).toBe(1);
    });

    test('leaves already $p()-wrapped voltage atoms untouched', () => {
        // Out of scope: only the calls being wrapped are normalized.
        const source = `$cycle($p("5v 3v"));`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
    });

    test('idempotent across a second migration of stripped output', () => {
        const once = migrateCycleCalls(`$cycle("5v 3v");`).migrated;
        const twice = migrateCycleCalls(once);
        expect(twice.migrated).toBe(once);
        expect(twice.callsChanged).toBe(0);
    });

    test('traces variable with single string assignment', () => {
        const source = `const pat = "c4 e4";\n$cycle(pat);`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(`const pat = $p("c4 e4");\n$cycle(pat);`);
        expect(result.callsChanged).toBe(0);
        expect(result.assignmentsChanged).toBe(1);
    });

    test('traces variable with multiple string assignments', () => {
        const source = `let p = "c4";\np = "e4";\n$cycle(p);`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `let p = $p("c4");\np = $p("e4");\n$cycle(p);`,
        );
        expect(result.assignmentsChanged).toBe(2);
    });

    test('skips variable with mixed assignments', () => {
        const source = `let p = "c4";\np = makePattern();\n$cycle(p);`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(source);
        expect(result.assignmentsChanged).toBe(0);
        expect(result.skippedVariables).toEqual(['p']);
    });

    test('migrates code in line comment', () => {
        const source = `// old: $cycle("c4")`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(`// old: $cycle($p("c4"))`);
        expect(result.commentsChanged).toBe(1);
    });

    test('migrates code in block comment', () => {
        const source = `/* draft: $cycle("c4 d4") */`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(`/* draft: $cycle($p("c4 d4")) */`);
        expect(result.commentsChanged).toBe(1);
    });

    test('wraps template literal first arg', () => {
        const source = '$cycle(`${root} e4`);';
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe('$cycle($p(`${root} e4`));');
        expect(result.callsChanged).toBe(1);
    });

    test('preserves quote style', () => {
        const src = `$cycle('c4');\n$cycle("d4");\n$cycle(\`e4\`);`;
        const result = migrateCycleCalls(src);
        expect(result.migrated).toBe(
            `$cycle($p('c4'));\n$cycle($p("d4"));\n$cycle($p(\`e4\`));`,
        );
        expect(result.callsChanged).toBe(3);
    });

    test('mixed buffer — calls + assignment + comment', () => {
        const source = [
            `const pat = "c4 e4";`,
            `$cycle(pat);`,
            `$cycle("g4");`,
            `// example: $cycle("a4")`,
        ].join('\n');
        const result = migrateCycleCalls(source);
        expect(result.callsChanged).toBe(1);
        expect(result.assignmentsChanged).toBe(1);
        expect(result.commentsChanged).toBe(1);
        expect(result.migrated).toContain(`const pat = $p("c4 e4");`);
        expect(result.migrated).toContain(`$cycle($p("g4"));`);
        expect(result.migrated).toContain(`// example: $cycle($p("a4"))`);
    });

    test('partial pre-migration — only unmigrated touched', () => {
        const source = `$cycle($p("c4"));\n$cycle("d4");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(`$cycle($p("c4"));\n$cycle($p("d4"));`);
        expect(result.callsChanged).toBe(1);
    });

    test('rewrites $iCycle(string, scale) to $cycle($p.s(string, scale))', () => {
        const source = `$iCycle("0 2 4", "C(major)");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `$cycle($p.s("0 2 4", "C(major)"));`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('rewrites $iCycle(array, scale) — chain via .add', () => {
        const source = `$iCycle(["0 2 4", "0,4", "5"], "C(major)");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `$cycle($p.s("0 2 4", "C(major)").add("0,4").add("5"));`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('rewrites $iCycle(single-elem array, scale) to plain $p.s', () => {
        const source = `$iCycle(["0 2 4"], "C(major)");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `$cycle($p.s("0 2 4", "C(major)"));`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('migrates $iCycle in line comment', () => {
        const source = `// $iCycle("0 2 4", "C")`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(`// $cycle($p.s("0 2 4", "C"))`);
        expect(result.commentsChanged).toBe(1);
    });

    test('migrates $iCycle array form in comment', () => {
        const source = `// $iCycle(["0 2", "4"], "C")`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `// $cycle($p.s("0 2", "C").add("4"))`,
        );
        expect(result.commentsChanged).toBe(1);
    });

    test('idempotent — already $p.s-form $iCycle migration unchanged', () => {
        const source = `$cycle($p.s("0 2 4", "C(major)"));`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
    });

    test('preserves scale variable identifier (no inlining)', () => {
        const source = `const scale = "C(major)";
$iCycle("0 2 4", scale);`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `const scale = "C(major)";
$cycle($p.s("0 2 4", scale));`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('preserves scale variable in array-form $iCycle', () => {
        const source = `const scale = "C(major)";
$iCycle(["0 2", "4"], scale);`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `const scale = "C(major)";
$cycle($p.s("0 2", scale).add("4"));`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('pushes $p.s into a reassigned string source variable', () => {
        const source = [
            `const key = 'c(maj)'`,
            ``,
            `let pat = '<0 2 4>*16'`,
            `pat = '<0 2 <4!2 5>>*16'`,
            ``,
            `const seq = $iCycle(pat, key)`,
        ].join('\n');
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            [
                `const key = 'c(maj)'`,
                ``,
                `let pat = $p.s('<0 2 4>*16', key)`,
                `pat = $p.s('<0 2 <4!2 5>>*16', key)`,
                ``,
                `const seq = $cycle(pat)`,
            ].join('\n'),
        );
        expect(result.callsChanged).toBe(1);
        expect(result.assignmentsChanged).toBe(2);
        expect(result.skippedVariables).toEqual([]);
    });

    test('idempotent — re-migrating pushed-down $p.s output is a no-op', () => {
        const source = [
            `const key = 'c(maj)'`,
            `let pat = '<0 2 4>*16'`,
            `pat = '<0 2 <4!2 5>>*16'`,
            `$iCycle(pat, key)`,
        ].join('\n');
        const once = migrateCycleCalls(source).migrated;
        const twice = migrateCycleCalls(once);
        expect(twice.migrated).toBe(once);
        expect(twice.callsChanged).toBe(0);
        expect(twice.assignmentsChanged).toBe(0);
        expect(twice.skippedVariables).toEqual([]);
    });

    test('pushes $p.s into a single string source variable', () => {
        const source = `const pat = "0 2 4";\n$iCycle(pat, "C");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `const pat = $p.s("0 2 4", "C");\n$cycle(pat);`,
        );
        expect(result.callsChanged).toBe(1);
        expect(result.assignmentsChanged).toBe(1);
        expect(result.skippedVariables).toEqual([]);
    });

    test('two $iCycle uses of one var with same scale share the push-down', () => {
        const source = [
            `let pat = "0 2";`,
            `$iCycle(pat, "C");`,
            `pat = "4 5";`,
            `$iCycle(pat, "C");`,
        ].join('\n');
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            [
                `let pat = $p.s("0 2", "C");`,
                `$cycle(pat);`,
                `pat = $p.s("4 5", "C");`,
                `$cycle(pat);`,
            ].join('\n'),
        );
        expect(result.assignmentsChanged).toBe(2);
        expect(result.callsChanged).toBe(2);
    });

    test('conflicting scales fall back to inline $p.s at each call', () => {
        const source = `let pat = "0 2";\n$iCycle(pat, "C");\n$iCycle(pat, "D");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `let pat = "0 2";\n$cycle($p.s(pat, "C"));\n$cycle($p.s(pat, "D"));`,
        );
        expect(result.assignmentsChanged).toBe(0);
        expect(result.callsChanged).toBe(2);
    });

    test('skips $iCycle source variable with a non-string assignment', () => {
        const source = `let pat = "0 2";\npat = makePat();\n$iCycle(pat, "C");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
        expect(result.skippedVariables).toEqual(['pat']);
    });

    test('garbage input — best-effort, no throw', () => {
        const source = `this is not valid js {{{`;
        const result = migrateCycleCalls(source);
        // ts-morph parses leniently; expect no edits, no throw.
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
    });

    test('overlapping edits in one comment abort migration cleanly', () => {
        // The outer $iCycle(...) edit and the inner $cycle("a") edit
        // both fire on overlapping ranges. applyEdits must report the
        // conflict, and the result must be source-identical with zeroed
        // counts and an error set — never a "nonzero counts but no diff"
        // state that would let the UI overwrite the buffer with itself.
        const source = `// $iCycle([$cycle("a")], "C")`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
        expect(result.assignmentsChanged).toBe(0);
        expect(result.commentsChanged).toBe(0);
        expect(result.error).toBeDefined();
    });

    test('does not rewrite a binding that only exists in an inner scope', () => {
        // The only `p` in source is declared inside `inner()`. The
        // top-level `$cycle(p)` references some other (undeclared) `p`
        // — collectAssignments must not treat the inner binding as a
        // hit just because the name matches.
        const source =
            'function inner(){ const p = "c4"; return p; }\n$cycle(p);';
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(source);
        expect(result.callsChanged).toBe(0);
        expect(result.assignmentsChanged).toBe(0);
    });
});
