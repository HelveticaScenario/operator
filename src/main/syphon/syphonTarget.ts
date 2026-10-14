/** Which window Syphon publishes: the editor, or the performance window. */
export type SyphonTarget = 'editor' | 'performance';

export type SyphonAction = 'start' | 'stop' | 'switch';

/**
 * What a toggle does. Naming a window (a menu checkbox) starts publishing it,
 * stops when it is already the one published, or moves publishing over from
 * the other. Naming none (the toggle command) stops whatever is published, or
 * starts the editor.
 */
export function syphonAction(
    enabled: boolean,
    current: SyphonTarget,
    requested?: SyphonTarget,
): SyphonAction {
    if (!enabled) return 'start';
    return requested === undefined || current === requested ? 'stop' : 'switch';
}
