import * as path from 'path';

/**
 * Resolve a caller-supplied file path to an absolute path, enforcing
 * workspace containment. Relative paths resolve against the workspace root;
 * absolute paths must already lie inside it. Returns null when there is no
 * workspace or the resolved path escapes it.
 */
export function resolveWorkspacePath(
    workspaceRoot: string | null,
    filePath: string,
): string | null {
    if (!workspaceRoot) {
        return null;
    }
    // path.resolve normalizes away any embedded '..' segments before the
    // containment check below.
    const resolved = path.isAbsolute(filePath)
        ? path.resolve(filePath)
        : path.resolve(workspaceRoot, filePath);

    const relative = path.relative(workspaceRoot, resolved);
    const escapesWorkspace =
        relative === '..' ||
        relative.startsWith(`..${path.sep}`) ||
        path.isAbsolute(relative);
    return escapesWorkspace ? null : resolved;
}
