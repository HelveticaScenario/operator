/**
 * Workbench-level command palette (cmdk).
 *
 * Rendered at the App root so it works regardless of whether a Monaco editor
 * is currently mounted. Sources its rows from `buildPaletteItems` — Operator
 * registry commands plus, when an editor exists, every supported Monaco
 * editor action.
 *
 * See `~/.claude/plans/operator-is-at-its-goofy-mist.md` Phase 2.1a.
 */
import { useMemo, useRef } from 'react';
import { Command } from 'cmdk';
import type { editor } from 'monaco-editor';

import { buildPaletteItems, type PaletteItem } from '../keybindings/paletteItems';
import './CommandPalette.css';

export interface CommandPaletteProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    /** Currently mounted Monaco editor, if any. Used to enumerate editor actions. */
    editor: editor.ICodeEditor | null;
}

/**
 * Stable item value for cmdk's filter. cmdk filters on this string, so we
 * concatenate category + label + id — so "Go to Line" matches, the category
 * ("editor") matches, and the internal id `editor.action.gotoLine` matches.
 */
function itemValue(item: PaletteItem): string {
    return `${item.category ?? ''} ${item.label} ${item.id}`;
}

// Display symbols for accelerator tokens, rendered as individual key chips.
const KEY_SYMBOLS: Record<string, string> = {
    Cmd: '⌘',
    Command: '⌘',
    Ctrl: '⌃',
    Control: '⌃',
    Alt: '⌥',
    Option: '⌥',
    Shift: '⇧',
    Enter: '↵',
    Return: '↵',
};

/** Split an Electron accelerator (`Cmd+Shift+P`) into display key chips. */
function keybindingChips(accelerator: string): string[] {
    return accelerator
        .split('+')
        .map((token) => KEY_SYMBOLS[token] ?? token);
}

export function CommandPalette({
    open,
    onOpenChange,
    editor,
}: CommandPaletteProps) {
    // Rebuild on every open so the registry and editor actions reflect the
    // current frame. The build is cheap (linear in command count) and only
    // runs when the user actually opens the palette.
    const items = useMemo(() => {
        if (!open) {
            return [];
        }
        return buildPaletteItems(editor);
    }, [open, editor]);

    const listRef = useRef<HTMLDivElement>(null);

    return (
        <Command.Dialog
            open={open}
            onOpenChange={onOpenChange}
            label="Command Palette"
            overlayClassName="command-palette-overlay"
            contentClassName="command-palette-content"
        >
            <Command.Input
                placeholder="Type a command…"
                className="command-palette-input"
                autoFocus
                onValueChange={() => {
                    // Editing the query re-ranks the list; jump back to the
                    // top so the best match is visible (cmdk keeps the prior
                    // scroll position otherwise).
                    listRef.current?.scrollTo({ top: 0 });
                }}
            />
            <Command.List ref={listRef} className="command-palette-list">
                <Command.Empty className="command-palette-empty">
                    No matching commands.
                </Command.Empty>
                {items.map((item) => (
                    <Command.Item
                        key={`${item.kind}:${item.id}`}
                        value={itemValue(item)}
                        onSelect={() => {
                            onOpenChange(false);
                            // Defer until the dialog (and its focus trap) has
                            // actually closed, so a command that opens its own
                            // focused UI — e.g. Go to Line's input — isn't
                            // immediately robbed of focus by the open palette.
                            requestAnimationFrame(() => item.run());
                        }}
                        className="command-palette-item"
                    >
                        <span className="command-palette-item-label">
                            {item.category && (
                                <span className="command-palette-item-category">
                                    {item.category}:{' '}
                                </span>
                            )}
                            {item.label}
                        </span>
                        {item.keybinding && (
                            <span className="command-palette-item-keys">
                                {keybindingChips(item.keybinding).map(
                                    (key, i) => (
                                        <kbd
                                            key={i}
                                            className="command-palette-key"
                                        >
                                            {key}
                                        </kbd>
                                    ),
                                )}
                            </span>
                        )}
                    </Command.Item>
                ))}
            </Command.List>
        </Command.Dialog>
    );
}
