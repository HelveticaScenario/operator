/**
 * Build the native editor context menu shown on right-click in Monaco.
 *
 * The menu merges, in lexical group order:
 *   1_patch        registry commands tagged with `contextMenu` (Update Patch …)
 *   2_navigation   Go to Definition / References / Symbol, and a Peek submenu
 *   3_modification Rename, Change All Occurrences, Format Document/Selection
 *   9_cutcopypaste native clipboard roles (the OS acts on the focused editor)
 *   z_commands     Command Palette (registry)
 *
 * Navigation/modification entries are core Monaco editor actions, dispatched
 * through `editor.trigger` (see `dispatch.ts`). Enabled state comes from the
 * editor when possible: actions registered via `registerEditorAction`
 * (Rename, Change All, Format) resolve through `getAction().isSupported()`.
 * The Go to / Peek navigation actions are `registerAction2` commands that
 * `getAction` does not expose, so their enabled state is read from the
 * matching provider context key (`requires`) instead. A separator is drawn
 * between groups; `null` marks a separator.
 */
import type { editor } from 'monaco-editor';

import type { ContextMenuItemDescriptor } from '../../shared/ipcTypes';
import { listCommands } from './commands';
import { evaluateWhen } from './contextKey';
import { getCommandAccelerator } from './keymap';

interface ActionSpec {
    id: string;
    label: string;
    group: string;
    order: number;
    /**
     * Provider context key gating enabled state, for `registerAction2`
     * navigation commands that `getAction` cannot resolve. Omitted for
     * actions whose support `getAction().isSupported()` reports directly.
     */
    requires?: string;
}

const NAVIGATION: ReadonlyArray<ActionSpec> = [
    {
        id: 'editor.action.revealDefinition',
        label: 'Go to Definition',
        group: '2_navigation',
        order: 1,
        requires: 'editorHasDefinitionProvider',
    },
    {
        id: 'editor.action.goToReferences',
        label: 'Go to References',
        group: '2_navigation',
        order: 2,
        requires: 'editorHasReferenceProvider',
    },
    {
        id: 'editor.action.quickOutline',
        label: 'Go to Symbol...',
        group: '2_navigation',
        order: 3,
        requires: 'editorHasDocumentSymbolProvider',
    },
];

// Children of the "Peek" submenu, nested under navigation.
const PEEK: ReadonlyArray<ActionSpec> = [
    {
        id: 'editor.action.peekDefinition',
        label: 'Peek Definition',
        group: 'peek',
        order: 1,
        requires: 'editorHasDefinitionProvider',
    },
    {
        id: 'editor.action.referenceSearch.trigger',
        label: 'Peek References',
        group: 'peek',
        order: 2,
        requires: 'editorHasReferenceProvider',
    },
];

const MODIFICATION: ReadonlyArray<ActionSpec> = [
    {
        id: 'editor.action.rename',
        label: 'Rename Symbol',
        group: '3_modification',
        order: 1,
    },
    {
        id: 'editor.action.changeAll',
        label: 'Change All Occurrences',
        group: '3_modification',
        order: 2,
    },
    {
        id: 'editor.action.formatDocument',
        label: 'Format Document',
        group: '3_modification',
        order: 3,
    },
    {
        id: 'editor.action.formatSelection',
        label: 'Format Selection',
        group: '3_modification',
        order: 4,
    },
];

const CLIPBOARD_ROLES: ReadonlyArray<{
    role: 'cut' | 'copy' | 'paste';
    order: number;
}> = [
    { role: 'cut', order: 1 },
    { role: 'copy', order: 2 },
    { role: 'paste', order: 3 },
];

type Placed = {
    group: string;
    order: number;
    descriptor: ContextMenuItemDescriptor;
};

/**
 * Resolve an editor-action spec against the live editor: the editor supplies
 * the canonical label and enabled state. If the action is not registered as
 * an editor action, it is still emitted (enabled) since `editor.trigger`
 * falls back to the command service for it.
 */
function actionDescriptor(
    ed: editor.ICodeEditor,
    spec: ActionSpec,
): ContextMenuItemDescriptor {
    const action = ed.getAction(spec.id);
    // Resolved editor actions report their own support; registerAction2
    // commands (getAction === null) fall back to their provider context key,
    // or to enabled when none is named.
    const enabled = action
        ? action.isSupported()
        : spec.requires
          ? evaluateWhen(spec.requires)
          : true;
    const accelerator = getCommandAccelerator(spec.id);
    return {
        kind: 'command',
        commandId: spec.id,
        label: action?.label?.trim() || spec.label,
        enabled,
        ...(accelerator ? { accelerator } : {}),
    };
}

/**
 * Build the ordered descriptor list for the editor context menu. The
 * clipboard roles are always present, so the result is never empty.
 */
export function buildEditorMenuItems(
    ed: editor.ICodeEditor,
): (ContextMenuItemDescriptor | null)[] {
    const placed: Placed[] = [];

    // Registry commands that placed themselves in the context menu.
    for (const { id, metadata } of listCommands()) {
        const placement = metadata?.contextMenu;
        if (!placement || !evaluateWhen(metadata?.when)) {
            continue;
        }
        const accelerator = getCommandAccelerator(id);
        placed.push({
            group: placement.group,
            order: placement.order,
            descriptor: {
                kind: 'command',
                commandId: id,
                label: metadata?.label ?? id,
                enabled: true,
                ...(accelerator ? { accelerator } : {}),
            },
        });
    }

    // Core Monaco navigation + modification actions.
    for (const spec of [...NAVIGATION, ...MODIFICATION]) {
        placed.push({
            group: spec.group,
            order: spec.order,
            descriptor: actionDescriptor(ed, spec),
        });
    }

    // Peek submenu, nested under navigation.
    placed.push({
        group: '2_navigation',
        order: NAVIGATION.length + 1,
        descriptor: {
            kind: 'submenu',
            label: 'Peek',
            items: PEEK.map((spec) => actionDescriptor(ed, spec)),
        },
    });

    // Native clipboard roles.
    for (const item of CLIPBOARD_ROLES) {
        placed.push({
            group: '9_cutcopypaste',
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
