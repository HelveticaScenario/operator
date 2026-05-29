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

    test('rewrites $iCycle(string, scale) to $cycle($sp(string, scale))', () => {
        const source = `$iCycle("0 2 4", "C(major)");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `$cycle($sp("0 2 4", "C(major)"));`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('rewrites $iCycle(array, scale) — chain via .add', () => {
        const source = `$iCycle(["0 2 4", "0,4", "5"], "C(major)");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `$cycle($sp("0 2 4", "C(major)").add("0,4").add("5"));`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('rewrites $iCycle(single-elem array, scale) to plain $sp', () => {
        const source = `$iCycle(["0 2 4"], "C(major)");`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `$cycle($sp("0 2 4", "C(major)"));`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('migrates $iCycle in line comment', () => {
        const source = `// $iCycle("0 2 4", "C")`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(`// $cycle($sp("0 2 4", "C"))`);
        expect(result.commentsChanged).toBe(1);
    });

    test('migrates $iCycle array form in comment', () => {
        const source = `// $iCycle(["0 2", "4"], "C")`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `// $cycle($sp("0 2", "C").add("4"))`,
        );
        expect(result.commentsChanged).toBe(1);
    });

    test('idempotent — already $sp-form $iCycle migration unchanged', () => {
        const source = `$cycle($sp("0 2 4", "C(major)"));`;
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
$cycle($sp("0 2 4", scale));`,
        );
        expect(result.callsChanged).toBe(1);
    });

    test('preserves scale variable in array-form $iCycle', () => {
        const source = `const scale = "C(major)";
$iCycle(["0 2", "4"], scale);`;
        const result = migrateCycleCalls(source);
        expect(result.migrated).toBe(
            `const scale = "C(major)";
$cycle($sp("0 2", scale).add("4"));`,
        );
        expect(result.callsChanged).toBe(1);
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
