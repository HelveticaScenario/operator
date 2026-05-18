/**
 * Keymap loader: merges user `keybindings.json` overrides onto the default
 * keymap and binds the result to `window` through `tinykeys`. Dispatch goes
 * through `executeCommand` so every surface (palette, context menu, OS menu,
 * keymap) shares one command path.
 *
 * Phase 2.3 from `~/.claude/plans/operator-is-at-its-goofy-mist.md`.
 *
 * When-clause evaluation is delegated through `setWhenEvaluator`. Phase 2.2
 * installs the real context-key parser; until then the default evaluator
 * treats every when-clause as true.
 */
import type { KeyBindingMap } from 'tinykeys';
import { tinykeys } from 'tinykeys';
import type { KeybindingOverride } from '../../shared/ipcTypes';
import { executeCommand, getCommand } from './commands';
import {
    DEFAULT_KEYMAP,
    defaultKeymapAsOverrides,
    type DefaultKeybinding,
} from './defaultKeymap';

export type ResolvedKeybinding = {
    key: string;
    command: string;
    when?: string;
    args?: unknown[];
};

type WhenEvaluator = (when: string | undefined) => boolean;

let whenEvaluator: WhenEvaluator = () => true;

/**
 * Replace the when-clause evaluator. Called by the context-key service
 * (Phase 2.2) once its parser is wired up. Tests use this to assert that
 * dispatch is gated correctly.
 */
export function setWhenEvaluator(evaluator: WhenEvaluator): void {
    whenEvaluator = evaluator;
}

function normalizeKey(key: string): string {
    return key.trim().toLowerCase();
}

/**
 * Merge user overrides on top of the default keymap.
 *
 * Semantics mirror VS Code's `keybindings.json`:
 *   - A user entry with `command: null` removes every default binding whose
 *     key matches (case-insensitive).
 *   - Other user entries are appended after the surviving defaults. Within a
 *     key, entries are tried in order at dispatch time; the first whose
 *     when-clause evaluates to true wins.
 */
export function mergeKeymap(
    defaults: readonly DefaultKeybinding[],
    userOverrides: readonly KeybindingOverride[],
): ResolvedKeybinding[] {
    const removedKeys = new Set<string>();
    for (const entry of userOverrides) {
        if (entry.command === null) {
            removedKeys.add(normalizeKey(entry.key));
        }
    }

    const out: ResolvedKeybinding[] = [];
    for (const entry of defaults) {
        if (removedKeys.has(normalizeKey(entry.key))) {
            continue;
        }
        out.push({
            key: entry.key,
            command: entry.command,
            ...(entry.when ? { when: entry.when } : {}),
        });
    }
    for (const entry of userOverrides) {
        if (entry.command === null) {
            continue;
        }
        out.push({
            key: entry.key,
            command: entry.command,
            ...(entry.when ? { when: entry.when } : {}),
            ...(entry.args ? { args: entry.args } : {}),
        });
    }
    return out;
}

/**
 * Group resolved entries by their tinykeys binding string. User overrides
 * later in the list shadow earlier (default) entries with the same key when
 * their when-clauses pass.
 */
function groupByKey(
    entries: readonly ResolvedKeybinding[],
): Map<string, ResolvedKeybinding[]> {
    const groups = new Map<string, ResolvedKeybinding[]>();
    for (const entry of entries) {
        const arr = groups.get(entry.key);
        if (arr) {
            arr.push(entry);
        } else {
            groups.set(entry.key, [entry]);
        }
    }
    // Later entries should be tried first so user overrides win over defaults
    // when when-clauses overlap.
    for (const arr of groups.values()) {
        arr.reverse();
    }
    return groups;
}

/**
 * Build the `tinykeys` binding map from grouped entries. Each handler walks
 * its candidates and fires the first whose when-clause passes; if none pass
 * the event is left alone so other listeners (e.g. Monaco) can handle it.
 */
function buildBindingMap(
    groups: Map<string, ResolvedKeybinding[]>,
): KeyBindingMap {
    const map: KeyBindingMap = {};
    for (const [key, candidates] of groups) {
        map[key] = (event) => {
            for (const candidate of candidates) {
                if (!whenEvaluator(candidate.when)) {
                    continue;
                }
                if (!getCommand(candidate.command)) {
                    // Unregistered command - skip rather than throw so a
                    // typo in user keybindings.json doesn't break dispatch.
                    console.warn(
                        `[keymap] binding "${key}" references unknown command "${candidate.command}"`,
                    );
                    continue;
                }
                event.preventDefault();
                event.stopPropagation();
                try {
                    const args = candidate.args ?? [];
                    void executeCommand(candidate.command, ...args);
                } catch (error) {
                    console.error(
                        `[keymap] error executing command "${candidate.command}":`,
                        error,
                    );
                }
                return;
            }
        };
    }
    return map;
}

export type InstallKeymapResult = {
    entries: ResolvedKeybinding[];
    dispose: () => void;
};

/**
 * Bind a fully-resolved keymap to the given target (defaults to `window`)
 * via `tinykeys`. Returns the list of entries that were actually wired plus
 * a disposer that detaches the listener.
 */
export function installKeymap(
    entries: readonly ResolvedKeybinding[],
    target: Window | HTMLElement = window,
): InstallKeymapResult {
    const groups = groupByKey(entries);
    const map = buildBindingMap(groups);
    const dispose = tinykeys(target, map);
    return { entries: [...entries], dispose };
}

/**
 * Bootstrap path: read user overrides through the preload bridge, merge
 * onto defaults, and install. Silent fallback to defaults if the preload
 * API is unavailable (e.g. when rendered outside Electron during tests).
 */
export async function loadAndInstallKeymap(
    target: Window | HTMLElement = window,
): Promise<InstallKeymapResult> {
    const overrides = await loadUserOverrides();
    const entries = mergeKeymap(DEFAULT_KEYMAP, overrides);
    return installKeymap(entries, target);
}

async function loadUserOverrides(): Promise<KeybindingOverride[]> {
    const api = (window as unknown as { electronAPI?: ElectronAPILike })
        .electronAPI;
    if (!api?.keybindings?.readUser) {
        return [];
    }
    try {
        return await api.keybindings.readUser();
    } catch (error) {
        console.error('[keymap] failed to read user keybindings:', error);
        return [];
    }
}

type ElectronAPILike = {
    keybindings?: {
        readUser?: () => Promise<KeybindingOverride[]>;
    };
};

// Re-export for callers that want to expose the active default set to UI
// (e.g. the read-only Keyboard Shortcuts tab in Phase 2.5).
export { DEFAULT_KEYMAP, defaultKeymapAsOverrides };
