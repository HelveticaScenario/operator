import * as path from 'path';

/**
 * Resolve a caller-supplied file path to an absolute path the filesystem IPC
 * handlers may touch, enforcing workspace containment. Relative paths resolve
 * against the workspace root; absolute paths must already lie inside it.
 * `allowedFiles` are exact absolute paths of app-managed files outside the
 * workspace (e.g. the user keybindings.json the editor opens directly) that
 * are permitted as-is. Returns null when there is no workspace or the
 * resolved path escapes it.
 */
export function resolveWorkspacePath(
    workspaceRoot: string | null,
    filePath: string,
    allowedFiles: readonly string[] = [],
): string | null {
    let resolved: string;
    if (path.isAbsolute(filePath)) {
        // Normalizes away any embedded '..' segments before the checks below.
        resolved = path.resolve(filePath);
    } else if (workspaceRoot) {
        resolved = path.resolve(workspaceRoot, filePath);
    } else {
        return null;
    }

    if (allowedFiles.includes(resolved)) {
        return resolved;
    }

    if (!workspaceRoot) {
        return null;
    }

    const relative = path.relative(workspaceRoot, resolved);
    const escapesWorkspace =
        relative === '..' ||
        relative.startsWith(`..${path.sep}`) ||
        path.isAbsolute(relative);
    return escapesWorkspace ? null : resolved;
}
