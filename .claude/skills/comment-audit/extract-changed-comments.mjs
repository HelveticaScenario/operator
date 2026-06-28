#!/usr/bin/env node
// Extracts comments on added/changed lines in the working tree (vs a base ref,
// default HEAD), plus all comments in untracked files. Output is a JSON array of
// comment units — consecutive standalone comment lines are grouped into one unit
// so a multi-line block is judged whole. Trailing comments (code then `//`) are
// their own units. Only the comment text is emitted; the code it annotates is not.
//
// Usage: node extract-changed-comments.mjs [--base <ref>] [--pretty]

import { execSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';

const argv = process.argv.slice(2);
const base = argv.includes('--base')
    ? argv[argv.indexOf('--base') + 1]
    : 'HEAD';
const pretty = argv.includes('--pretty');

const CODE_EXT = new Set(['rs', 'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs']);

function sh(cmd) {
    return execSync(cmd, { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 });
}

function trysh(cmd) {
    try {
        return sh(cmd);
    } catch {
        return '';
    }
}

const tracked = trysh(`git diff --name-only ${base} --`)
    .split('\n')
    .filter(Boolean);
const untracked = trysh(`git ls-files --others --exclude-standard`)
    .split('\n')
    .filter(Boolean);
const untrackedSet = new Set(untracked);

const files = [...new Set([...tracked, ...untracked])].filter((f) =>
    CODE_EXT.has(f.split('.').pop()),
);

// Set of 1-based line numbers in the new file that were added or changed.
function addedLines(file) {
    if (untrackedSet.has(file)) {
        if (!existsSync(file)) return null;
        const n = readFileSync(file, 'utf8').split('\n').length;
        const s = new Set();
        for (let i = 1; i <= n; i++) s.add(i);
        return s;
    }
    const diff = trysh(`git diff -U0 ${base} -- "${file}"`);
    const s = new Set();
    let newLine = 0;
    for (const line of diff.split('\n')) {
        const m = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
        if (m) {
            newLine = parseInt(m[1], 10);
            continue;
        }
        if (line.startsWith('+') && !line.startsWith('+++')) {
            s.add(newLine);
            newLine++;
        }
    }
    return s;
}

// End index (just past the closing quote) of the Rust char/byte literal opening at
// `line[i]`, or -1 when that `'` is a lifetime/label (`'a`, `'static`) or not a complete
// single-line literal. Consuming the literal whole keeps a quote in its body (e.g. the
// double-quote byte `b'"'`) from being mistaken for a string delimiter.
function rustCharLitEnd(line, i) {
    if (line[i + 1] === '\\') {
        // Escaped body of variable length (`\n`, `\xFF`, `\u{1F600}`): the char after the
        // backslash is literal, then scan to the closing quote.
        let k = i + 3;
        while (k < line.length && line[k] !== "'") k++;
        return line[k] === "'" ? k + 1 : -1;
    }
    // Single unescaped char: `'<c>'`.
    if (line[i + 1] !== undefined && line[i + 1] !== "'" && line[i + 2] === "'")
        return i + 3;
    return -1;
}

// Per-line comment scan. Block-comment depth, template literals (with `${ … }` interpolation
// parsed as code), raw strings, and `"` strings span lines, so `//` inside any of them — or a
// `/* ... */` / multi-line literal — is not mistaken for a comment. A `"` string carries to the
// next line as a Rust multi-line literal or a JS trailing-backslash continuation; a JS `'`/`"`
// left open with no continuation resets at end of line so a stray quote cannot cascade. Rust
// char/byte literals (`'a'`, `b'"'`), raw strings (`r#"..."#`), nested block comments, and `'a`
// lifetimes are handled. JS regex literals are not recognized: a `//` inside one is a stray
// flagged comment, and a backtick inside one suppresses comments until the next backtick —
// both rare, and the reviewer drops the stray.
function scan(content, isRust) {
    const lines = content.split('\n');
    const out = []; // { hasComment, codeBefore, text } per line (1-based via shift)
    let depth = 0; // block-comment depth
    // JS template-literal nesting: a stack of `{ kind: 'text' }` (between backticks) and
    // `{ kind: 'interp', braces }` (inside `${ … }`, where `braces` counts nested `{}` so the
    // interpolation ends on the `}` matching its `${`). Parsing interpolation as real code
    // keeps a backtick inside an interpolation string (e.g. ``${esc("`")}``) from being
    // mistaken for the template's closing backtick and desyncing every following line.
    const tstk = [];
    let str = null; // active string delimiter (' or "); a `"` may span lines
    let rawHashes = -1; // inside a Rust raw string: number of `#` needed to close it, else -1
    for (let ln = 0; ln < lines.length; ln++) {
        const line = lines[ln];
        // Continuing a string, raw string, template, or interpolation from a prior line means
        // code already precedes anything on this line.
        let codeBefore = tstk.length > 0 || str !== null || rawHashes >= 0;
        const parts = [];
        let j = 0;
        let cont = false; // line ends mid-string on a `\` (JS line continuation)
        while (j < line.length) {
            const c = line[j];
            const c2 = line[j + 1];
            const top = tstk[tstk.length - 1];
            if (depth > 0) {
                if (c === '*' && c2 === '/') {
                    depth--;
                    j += 2;
                    continue;
                }
                if (isRust && c === '/' && c2 === '*') {
                    depth++;
                    j += 2;
                    continue;
                }
                parts.push(c);
                j++;
                continue;
            }
            if (top && top.kind === 'text') {
                // Template-literal text: opaque except its closing backtick, a `\` escape, and
                // the start of an interpolation.
                if (c === '`') {
                    tstk.pop();
                    j++;
                } else if (c === '\\') {
                    j += 2;
                } else if (c === '$' && c2 === '{') {
                    tstk.push({ kind: 'interp', braces: 0 });
                    j += 2;
                } else {
                    j++;
                }
                continue;
            }
            if (rawHashes >= 0) {
                // Raw-string body is verbatim; it closes only on `"` followed by the opening
                // run of `#`.
                if (c === '"') {
                    let closed = true;
                    for (let h = 1; h <= rawHashes; h++)
                        if (line[j + h] !== '#') {
                            closed = false;
                            break;
                        }
                    if (closed) {
                        j += 1 + rawHashes;
                        rawHashes = -1;
                        continue;
                    }
                }
                j++;
                continue;
            }
            if (str) {
                if (c === '\\') {
                    if (j + 1 >= line.length) cont = true; // trailing backslash → continues on next line
                    j += 2;
                    continue;
                }
                if (c === str) str = null;
                j++;
                continue;
            }
            if (c === '/' && c2 === '/') {
                parts.push(line.slice(j + 2));
                break;
            }
            if (c === '/' && c2 === '*') {
                depth++;
                j += 2;
                continue;
            }
            // Rust raw string `r"..."` / `r#"..."#` / `br#"..."#`: the `r` (or `br`) must start a
            // token, and the body is verbatim, so quotes and `//` inside it are inert.
            if (
                isRust &&
                (c === 'r' || (c === 'b' && c2 === 'r')) &&
                !/[A-Za-z0-9_]/.test(line[j - 1] || '')
            ) {
                let k = c === 'b' ? j + 2 : j + 1;
                let h = 0;
                while (line[k] === '#') {
                    h++;
                    k++;
                }
                if (line[k] === '"') {
                    rawHashes = h;
                    codeBefore = true;
                    j = k + 1;
                    continue;
                }
            }
            if (!isRust && c === '`') {
                tstk.push({ kind: 'text' }); // open a template literal
                codeBefore = true;
                j++;
                continue;
            }
            if (isRust && c === "'") {
                // A complete char/byte literal is consumed whole; otherwise the `'` is a
                // lifetime/label and only it is skipped. Rust has no `'`-delimited strings.
                const end = rustCharLitEnd(line, j);
                codeBefore = true;
                j = end >= 0 ? end : j + 1;
                continue;
            }
            if (c === '"' || c === "'") {
                str = c;
                codeBefore = true;
                j++;
                continue;
            }
            if (top && top.kind === 'interp' && (c === '{' || c === '}')) {
                // Balance braces so the interpolation ends on the `}` matching its `${`.
                if (c === '}' && top.braces === 0) tstk.pop();
                else if (c === '{') top.braces++;
                else top.braces--;
                codeBefore = true;
                j++;
                continue;
            }
            if (!/\s/.test(c)) codeBefore = true;
            j++;
        }
        // An unterminated string carries to the next line only when it legitimately spans one:
        // a Rust `"` literal, or a JS string left open by a trailing `\`. Anything else (a JS
        // `'`, or a `"` with no continuation) is reset so a stray quote cannot cascade past this
        // line. Raw strings and templates legitimately span lines and are never reset here.
        if (str !== null && !(isRust ? str === '"' : cont)) str = null;
        // Lines wholly inside a block comment accumulate their characters into `parts`,
        // so a non-empty `parts` is the test for "this line carries comment text".
        out.push({
            hasComment: parts.length > 0,
            codeBefore,
            text: parts.join('').trim(),
        });
    }
    return out;
}

const result = [];
for (const file of files) {
    if (!existsSync(file)) continue;
    const added = addedLines(file);
    if (!added) continue;
    const isRust = file.endsWith('.rs');
    const info = scan(readFileSync(file, 'utf8'), isRust);

    // Group consecutive standalone comment lines into one unit; trailing comments stand alone.
    const units = [];
    let cur = null;
    const flush = () => {
        if (cur) units.push(cur);
        cur = null;
    };
    for (let i = 0; i < info.length; i++) {
        const ln = i + 1;
        const it = info[i];
        if (!it.hasComment || !it.text) {
            flush();
            continue;
        }
        if (it.codeBefore) {
            flush();
            units.push({
                type: 'trailing',
                start: ln,
                end: ln,
                lines: [it.text],
            });
            continue;
        }
        if (cur && cur.type === 'standalone' && cur.end === ln - 1) {
            cur.end = ln;
            cur.lines.push(it.text);
        } else {
            flush();
            cur = { type: 'standalone', start: ln, end: ln, lines: [it.text] };
        }
    }
    flush();

    for (const u of units) {
        let touched = false;
        for (let ln = u.start; ln <= u.end; ln++)
            if (added.has(ln)) touched = true;
        if (!touched) continue;
        result.push({
            file,
            start: u.start,
            end: u.end,
            type: u.type,
            text: u.lines.join('\n'),
        });
    }
}

process.stdout.write(JSON.stringify(result, null, pretty ? 2 : 0) + '\n');
