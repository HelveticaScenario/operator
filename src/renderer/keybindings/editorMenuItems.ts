/**
 * Build the native editor context menu shown on right-click in Monaco.
 *
 * Two sources are merged each time the menu opens:
 *   1. Registry commands that opted in via `metadata.contextMenu` and whose
 *      `when`-clause currently evaluates to true.
 *   2. The clipboard trio (cut / copy / paste), emitted as native Electron
 *      menu roles so the OS performs them against the focused editor. Monaco
 *      registers clipboard as commands, not editor actions, so they are not
 *      reachable via `editor.getAction`; the native role is both simpler and
 *      the only thing that actually works here.
 *
 * Entries are grouped by `contextMenu.group` (Monaco-style lexical group ids
 * such as `1_patch`, `9_cutcopypaste`) and ordered within each group. A
 * separator is emitted between groups. The result is a flat descriptor list
 * the main process turns into a native `Menu`; `null` marks a separator.
 *
 * Phase 2.1b: the editor context menu surface from
 * `~/.claude/plans/operator-is-at-its-goofy-mist.md`.
 */
import type { ContextMenuItemDescriptor } from '../../shared/ipcTypes';
import { listCommands } from './commands';
import { evaluateWhen } from './contextKey';

/**
 * Clipboard roles surfaced in every editor menu, in display order. Electron
 * supplies their labels, accelerators, and enabled state.
 */
const CLIPBOARD_ROLES: ReadonlyArray<{
    role: 'cut' | 'copy' | 'paste';
    group: string;
    order: number;
}> = [
    { role: 'cut', group: '9_cutcopypaste', order: 1 },
    { role: 'copy', group: '9_cutcopypaste', order: 2 },
    { role: 'paste', group: '9_cutcopypaste', order: 3 },
];

type Placed = {
    group: string;
    order: number;
    descriptor: ContextMenuItemDescriptor;
};

/**
 * Build the ordered descriptor list for the editor context menu. The
 * clipboard roles are always present, so the result is never empty.
 */
export function buildEditorMenuItems(): (ContextMenuItemDescriptor | null)[] {
    const placed: Placed[] = [];

    // 1. Registry commands that placed themselves in the context menu.
    for (const { id, metadata } of listCommands()) {
        const placement = metadata?.contextMenu;
        if (!placement) {
            continue;
        }
        if (!evaluateWhen(metadata?.when)) {
            continue;
        }
        placed.push({
            group: placement.group,
            order: placement.order,
            descriptor: {
                kind: 'command',
                commandId: id,
                label: metadata?.label ?? id,
                enabled: true,
            },
        });
    }

    // 2. Native clipboard roles.
    for (const item of CLIPBOARD_ROLES) {
        placed.push({
            group: item.group,
            order: item.order,
            descriptor: { kind: 'role', role: item.role },
        });
    }

    // Group ids sort lexically (Monaco convention); order breaks ties.
    placed.sort((a, b) =>
        a.group === b.group ? a.order - b.order : a.group < b.group ? -1 : 1,
    );

    const out: (ContextMenuItemDescriptor | null)[] = [];
    let lastGroup: string | null = null;
    for (const entry of placed) {
        if (lastGroup !== null && entry.group !== lastGroup) {
            out.push(null);
        }
        out.push(entry.descriptor);
        lastGroup = entry.group;
    }
    return out;
}
