/**
 * Pieces shared by every control in the panel: the Ctrl/Cmd "go to code"
 * gesture, the out-of-sync marker, and the tooltip that explains both.
 */

import { useEffect, useState } from 'react';
import type { ControlBinding } from '../../app/controlBinding';

const JUMP_HINT = '⌃/⌘-click: go to code';

function isJumpGesture(e: React.MouseEvent | React.PointerEvent): boolean {
    return e.ctrlKey || e.metaKey;
}

/**
 * A Ctrl/Cmd-click on a control moves the editor cursor inside its call
 * instead of operating the control. The handlers run in the capture phase so
 * the control never sees the gesture; on macOS Ctrl+click also raises a
 * context menu, which is suppressed.
 */
export function jumpHandlers(offset: number, onJump: (offset: number) => void) {
    return {
        onClickCapture: (e: React.MouseEvent) => {
            if (isJumpGesture(e)) {
                e.preventDefault();
                e.stopPropagation();
            }
        },
        onContextMenu: (e: React.MouseEvent) => {
            if (e.ctrlKey) {
                e.preventDefault();
            }
        },
        onMouseDownCapture: (e: React.MouseEvent) => {
            if (isJumpGesture(e)) {
                e.preventDefault();
                e.stopPropagation();
            }
        },
        onPointerDownCapture: (e: React.PointerEvent) => {
            if (isJumpGesture(e)) {
                e.preventDefault();
                e.stopPropagation();
                onJump(offset);
            }
        },
    };
}

/**
 * Whether Ctrl/Cmd is held, so the hovered control can show it is a jump
 * target. Window listeners run in the capture phase so the editor, which
 * usually has focus, cannot swallow the keys; losing focus clears the state
 * since the key-up may land in another window.
 */
export function useJumpModifier(): [boolean, (held: boolean) => void] {
    const [held, setHeld] = useState(false);
    useEffect(() => {
        const onKey = (e: KeyboardEvent) => setHeld(e.ctrlKey || e.metaKey);
        const onBlur = () => setHeld(false);
        window.addEventListener('keydown', onKey, true);
        window.addEventListener('keyup', onKey, true);
        window.addEventListener('blur', onBlur);
        return () => {
            window.removeEventListener('keydown', onKey, true);
            window.removeEventListener('keyup', onKey, true);
            window.removeEventListener('blur', onBlur);
        };
    }, []);
    return [held, setHeld];
}

/** Tooltip for a control: why it is out of sync, if it is, plus the jump hint. */
export function controlTooltip(message: string | undefined): string {
    return message ? `${message}\n${JUMP_HINT}` : JUMP_HINT;
}

/** Why a control does not match the running patch, or undefined if it does. */
export function unsyncedMessage(control: ControlBinding): string | undefined {
    if (control.incomplete) {
        return 'Arguments are incomplete — finish editing to use this control';
    }
    if (control.synced) {
        return undefined;
    }
    return control.moduleId !== null
        ? 'Differs from the running patch — evaluate to apply'
        : 'Not in the running patch — evaluate to apply';
}

/** Shown on a control the running patch does not match — one not yet
 *  evaluated, in a buffer that is not running, edited since evaluation, or
 *  with arguments mid-edit. */
export function UnsyncedMarker({ message }: { message: string }) {
    return <span className="control-unsynced" title={message} />;
}
