/**
 * Operator command registry.
 *
 * Global, process-wide map of command id -> handler + metadata. Modeled
 * after VS Code's `CommandsRegistry`. Pure JS: no Monaco / Electron coupling
 * lives in this file.
 *
 * Surfaces that dispatch commands (cmdk palette, editor context menu,
 * tinykeys keymap, Electron menu IPC, in-editor key bindings) all funnel
 * through `executeCommand` so the body of each operation lives in exactly
 * one place.
 *
 * See `~/.claude/plans/operator-is-at-its-goofy-mist.md` Phase 2.1.
 */

export type CommandHandler = (...args: unknown[]) => void | Promise<void>;

export type CommandMetadata = {
    /** Human-readable label shown in the palette / context menu. */
    label: string;
    /** Optional grouping label, e.g. "Patch", "File". */
    category?: string;
    /**
     * Optional when-clause string. Stored verbatim here; parsed and
     * evaluated against the context-key service in Phase 2.2.
     */
    when?: string;
    /**
     * Optional placement in the editor context menu, consumed by
     * `editorMenuItems.ts`. Entries sort by `group` (a Monaco-style lexical
     * id such as `1_patch` or `9_cutcopypaste`) then `order`, with a
     * separator drawn between groups.
     */
    contextMenu?: { group: string; order: number };
};

type RegistryEntry = {
    handler: CommandHandler;
    metadata?: CommandMetadata;
};

/**
 * Internal singleton. Not exported: callers must go through the
 * functional API so we can later add change events, scopes, etc.
 */
const commandRegistry: Map<string, RegistryEntry> = new Map();

/**
 * Register (or replace) a command. Replacing an existing id logs a
 * `console.warn` to help catch accidental duplicate registrations across
 * modules.
 */
export function registerCommand(
    id: string,
    handler: CommandHandler,
    metadata?: CommandMetadata,
): void {
    if (commandRegistry.has(id)) {
        console.warn(
            `[commands] overwriting existing command registration for "${id}"`,
        );
    }
    commandRegistry.set(id, { handler, metadata });
}

/**
 * Unregister a command. No-op if the id is not registered. Returns
 * whether anything was removed (useful for cleanup assertions in tests).
 */
export function unregisterCommand(id: string): boolean {
    return commandRegistry.delete(id);
}

/**
 * Look up a command without invoking it. Returns `undefined` for unknown
 * ids — callers that want a strict lookup should use `executeCommand`.
 */
export function getCommand(id: string): RegistryEntry | undefined {
    return commandRegistry.get(id);
}

/**
 * Invoke a registered command. Throws if the id is not registered; the
 * thrown error includes the id so dispatch sites surface useful messages.
 *
 * Returns whatever the handler returns (commonly `void` or a Promise),
 * so async commands can be awaited at the call site.
 */
export function executeCommand(id: string, ...args: unknown[]): unknown {
    const entry = commandRegistry.get(id);
    if (!entry) {
        throw new Error(`[commands] unknown command id: "${id}"`);
    }
    return entry.handler(...args);
}

/**
 * Snapshot of every registered command. Used to populate the cmdk
 * palette and the read-only "Keyboard Shortcuts" settings tab.
 */
export function listCommands(): Array<{ id: string; metadata?: CommandMetadata }> {
    const out: Array<{ id: string; metadata?: CommandMetadata }> = [];
    for (const [id, entry] of commandRegistry) {
        out.push({ id, metadata: entry.metadata });
    }
    return out;
}
