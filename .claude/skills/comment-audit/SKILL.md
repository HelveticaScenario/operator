---
name: comment-audit
description: >
  Audit the comments on changed/added lines in the working tree against the project's
  comment guidelines (present-tense artifact: no history, no roads-not-taken; short,
  why-only, no over-commenting). Judges comment text alone — fast and cheap, no code
  reading. Use when the user asks to audit/check comments, before committing, or says
  "/comment-audit". Does NOT judge whether a comment accurately describes its code
  (that needs the code — use a full review for logic-narration).
---

Audit only the **comments on lines that changed in the working tree**, against the
`Comments` convention in `CLAUDE.md`. This is a stylistic, text-only pass: every check
below is decidable from the comment text alone, so it runs on a cheap model and never
reads the code the comment annotates.

**Out of scope (do not attempt here):** whether a comment is accurate, or narrates the
adjacent logic too closely. That requires reading the code and belongs in a full review.

## Steps

1. **Extract.** Run the deterministic extractor from the repo root:

   ```bash
   node .claude/skills/comment-audit/extract-changed-comments.mjs
   ```

   It prints a JSON array of comment units `{ file, start, end, type, text }` (`type`
   is `standalone` or `trailing`; consecutive comment lines are grouped into one unit;
   only comment text is emitted, never the code). Pass `--base <ref>` to audit against a
   ref other than `HEAD`. If the array is empty, report "No changed comments to audit."
   and stop.

2. **Judge — on Haiku.** Spawn **one** agent with `model: haiku` (the task is bounded
   text classification; Haiku is fast, cheap, and sufficient). Give it the JSON array and
   the rules below, and have it return a JSON array of findings. Do not read any source
   files — the comment text is all it needs. For a very large array (>200 units), split
   across a few Haiku agents and merge.

3. **Report.** Present findings grouped by file, each as
   `path:start-end  [rule]  "offending text"  → fix`. Lead with `must-fix` findings, then
   `nit`. End with a one-line tally. Then offer to apply the fixes to the working tree.

## Rules the Haiku judge applies

A unit violates the guidelines if any rule below fires. Severity is `must-fix` for the
hard invariant (history / roads-not-taken / commit refs / commented-out code), `nit` for
the rest.

**`history` (must-fix) — the codebase is a present-tense artifact.** The comment refers
to a past state or change. Triggers: "used to", "now"/"now we", "previously", "formerly",
"no longer", "renamed", "moved from", "was X", "originally", "changed to", or any
narration of how the code got here. Fix: restate as what is true now, or delete.

**`roads-not-taken` (must-fix).** The comment mentions an alternative considered, a
rejected approach, a decision, or a bug that once existed. Triggers: "instead of",
"rather than", "we chose", "could also", "avoids the old", "to fix the bug where". Fix:
state the invariant/why that holds now, with no reference to the alternative; or delete.

**`history-ref` (must-fix).** The comment cites a commit, PR, issue, or ticket
("PR #42", "see commit", "fixes #123", a Jira-style key). History lives in git only. Fix:
inline the actual reason in present tense, or delete the reference.

**`commented-out-code` (must-fix).** The comment body is disabled code rather than prose.
Fix: delete it.

**`unowned-todo` (must-fix).** A `TODO`/`FIXME`/`HACK`/`XXX` with no owner or tracked
follow-up. Fix: remove, or attach a concrete owner/reference.

**`too-long` (nit).** More than ~2 sentences, or padded with jargon or detail a reader
doesn't need to use the code. Comments should be short and complete. Fix: tighten to one
or two sentences.

**`not-a-sentence` (nit).** Sentence-fragment noise where a short complete sentence is
warranted (does not apply to terse field/param labels, which are fine). Fix: rewrite as a
complete sentence, or drop it.

**`low-value` (nit).** The comment adds nothing a careful reader wouldn't already know
from names and structure (pure restatement of an obvious construct, e.g. "constructor"
above a constructor). Fix: delete.

Do not flag a unit just because it is a doc comment, is technical, or explains a real
non-obvious *why* — those are the comments worth keeping.

**Never fabricate a rationale in a `fix`.** You only see the comment text, not the code or
the real reason. If a present-tense rewrite is recoverable from the text itself, propose
it; otherwise the `fix` is `"delete"` or `"delete, or restate the real reason in present
tense"` — never a plausible-sounding why you cannot actually know.

### Finding shape the judge returns

```json
[
  {
    "file": "src/main/foo.ts",
    "start": 12, "end": 14,
    "rule": "history",
    "severity": "must-fix",
    "text": "<the offending comment text>",
    "fix": "<concrete rewrite, or \"delete\">"
  }
]
```
