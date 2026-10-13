/** Which window Syphon publishes: the editor, or the performance window. */
export type SyphonTarget = 'editor' | 'performance';

export type SyphonAction = 'start' | 'stop' | 'switch';

/**
 * What choosing `requested` in the menu does: start publishing it, stop when
 * it is already the one published, or move publishing over from the other.
 */
export function syphonAction(
    enabled: boolean,
    current: SyphonTarget,
    requested: SyphonTarget,
): SyphonAction {
    if (!enabled) return 'start';
    return current === requested ? 'stop' : 'switch';
}
