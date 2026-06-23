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
import { dispatchCommand } from './dispatch';
import {
    DEFAULT_KEYMAP,
    defaultKeymapAsOverrides,
    type DefaultKeybinding,
} from './defaultKeymap';
import {
    normalizeOverride,
    toTinykeys,
    type Platform,
} from './vscodeKeys';
import { toElectronAccelerator } from '../../shared/keybindings/accelerator';

export type ResolvedKeybinding = {
    key: string;
    command: string;
    when?: string;
    args?: unknown;
};

function detectPlatform(): Platform {
    const api = (window as unknown as { electronAPI?: { platform?: string } })
        .electronAPI;
    return api?.platform === 'darwin' ? 'darwin' : 'other';
}

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

/**
 * Merge user overrides on top of the default keymap. Both sides are
 * translated to canonical tinykeys bindings (see `vscodeKeys`), so a default
 * and an override that denote the same physical chord collapse to one key.
 *
 * Semantics mirror VS Code's `keybindings.json`, applied in file order:
 *   - A removal (`command: null`, or a `-`-prefixed command) drops every
 *     earlier binding — default or prior override — with the same key, and
 *     the same command when one is named.
 *   - Other entries are appended. Within a key, the last entry wins (tried
 *     first at dispatch); the first whose when-clause passes fires.
 */
export function mergeKeymap(
    defaults: readonly DefaultKeybinding[],
    userOverrides: readonly KeybindingOverride[],
    platform: Platform,
): ResolvedKeybinding[] {
    let list: ResolvedKeybinding[] = [];
    for (const entry of defaults) {
        const key = toTinykeys(entry.key, platform);
        if (key === null) {
            continue;
        }
        list.push({
            key,
            command: entry.command,
            ...(entry.when ? { when: entry.when } : {}),
        });
    }
    for (const override of userOverrides) {
        const entry = normalizeOverride(override, platform);
        if (!entry) {
            continue;
        }
        if (entry.type === 'remove') {
            list = list.filter(
                (e) =>
                    !(
                        e.key === entry.key &&
                        (entry.command === null || e.command === entry.command)
                    ),
            );
        } else {
            list.push({
                key: entry.key,
                command: entry.command,
                ...(entry.when ? { when: entry.when } : {}),
                ...(entry.args !== undefined ? { args: entry.args } : {}),
            });
        }
    }
    return list;
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
                let handled = false;
                try {
                    // Resolves to an Operator command or, failing that, an
                    // editor command on the focused editor. Returns false when
                    // neither applies (no registry command, and no editor with
                    // text focus), so the event falls through to other
                    // listeners — keystrokes are never swallowed for an editor
                    // command while focus is elsewhere.
                    handled = dispatchCommand(candidate.command, candidate.args, {
                        requireEditorFocus: true,
                    });
                } catch (error) {
                    console.error(
                        `[keymap] error executing command "${candidate.command}":`,
                        error,
                    );
                    handled = true;
                }
                if (!handled) {
                    continue;
                }
                event.preventDefault();
                event.stopPropagation();
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
    commandBindings = buildCommandBindings(entries);
    const groups = groupByKey(entries);
    const map = buildBindingMap(groups);
    // Capture phase: Monaco's editor keybindings listen on the editor DOM node
    // in the bubble phase and stopPropagation() the keys they handle, which
    // would otherwise shadow our window-level bindings (e.g. Cmd+Enter, which
    // Monaco defaults to "insert line below"). Listening in capture lets an
    // Operator binding intercept its key before Monaco; keys we don't bind, or
    // don't handle (when-clause false), fall through untouched.
    const dispose = tinykeys(target, map, { capture: true });
    return { entries: [...entries], dispose };
}

// Command id -> resolved tinykeys binding (e.g. `Meta+Enter`, `Meta+k Meta+i`),
// rebuilt on each install. Single source of truth for the shortcut shown next
// to a command. The raw binding (chord-capable) drives the palette's key
// chips; the application menu derives a single-combo Electron accelerator.
let commandBindings: Record<string, string> = {};

function buildCommandBindings(
    entries: readonly ResolvedKeybinding[],
): Record<string, string> {
    const map: Record<string, string> = {};
    // Last binding for a command wins (user overrides come after defaults).
    for (const entry of entries) {
        map[entry.command] = entry.key;
    }
    return map;
}

/** Resolved tinykeys binding for a command, if any (chord-capable). */
export function getCommandBinding(commandId: string): string | undefined {
    return commandBindings[commandId];
}

/**
 * Electron accelerator for a command's binding, if it is a single combo
 * (Electron accelerators cannot express chord sequences) — used by the
 * application menu.
 */
export function getCommandAccelerator(commandId: string): string | undefined {
    const binding = commandBindings[commandId];
    return binding ? (toElectronAccelerator(binding) ?? undefined) : undefined;
}

/** Snapshot of every command's Electron accelerator (for the application menu). */
export function getCommandAccelerators(): Record<string, string> {
    const out: Record<string, string> = {};
    for (const [id, binding] of Object.entries(commandBindings)) {
        const accel = toElectronAccelerator(binding);
        if (accel) {
            out[id] = accel;
        }
    }
    return out;
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
    const entries = mergeKeymap(DEFAULT_KEYMAP, overrides, detectPlatform());
    const result = installKeymap(entries, target);
    // Push resolved accelerators to the main process so the application (top
    // bar) menu shows the same shortcuts as the editor context menu.
    const api = (
        window as unknown as {
            electronAPI?: {
                menu?: { setAccelerators?: (m: Record<string, string>) => void };
            };
        }
    ).electronAPI;
    api?.menu?.setAccelerators?.(getCommandAccelerators());
    return result;
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
