/**
 * Ctrl/Cmd gestures edit the code only, leaving the running audio as is
 * until the next patch run; the control then shows a ghost of the code-side
 * value. On macOS Ctrl+click also fires a contextmenu event, so controls
 * read a contextmenu that arrives during a press as part of that press.
 */
export function isCodeOnly(e: { ctrlKey: boolean; metaKey: boolean }): boolean {
    return e.ctrlKey || e.metaKey;
}

/** Fine-adjust drag scale while Shift is held. */
export const FINE_SCALE = 0.1;

/** A gesture step, or its end (which flushes the code write). */
export type EqChangePhase = 'move' | 'commit';
