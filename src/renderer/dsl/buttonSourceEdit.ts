/**
 * Utility for finding the initial-state literal in `$toggleBtn(label, state)`
 * calls so the UI can rewrite it via Monaco edits. Uses the same lightweight,
 * comment/string-aware scanning as slider source editing.
 *
 * This runs in the renderer process, so it must not depend on Node.js-only modules.
 */

import {
    findLabeledCallSecondArgStart,
    type SourceSpanResult,
} from './sliderSourceEdit';

/**
 * Find the character offset range of the `true`/`false` initial-state literal
 * in a `$toggleBtn(label, state)` call whose label matches the given string.
 *
 * @param source - The full DSL source code
 * @param label  - The label string to match against
 * @returns The start/end offsets of the state literal, or null if not found
 */
export function findToggleBtnStateSpan(
    source: string,
    label: string,
): SourceSpanResult | null {
    const start = findLabeledCallSecondArgStart(source, '$toggleBtn', label);
    if (start === null) {
        return null;
    }

    const boolMatch = source.slice(start).match(/^(true|false)\b/);
    if (!boolMatch) {
        return null;
    }

    return {
        end: start + boolMatch[0].length,
        start,
    };
}
