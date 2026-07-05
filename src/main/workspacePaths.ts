import * as path from 'path';

/**
 * Resolve a caller-supplied file path for the filesystem IPC handlers:
 * absolute paths pass through as-is, relative paths resolve against the
 * workspace root. Returns null for a relative path when there is no
 * workspace.
 */
export function resolveWorkspacePath(
    workspaceRoot: string | null,
    filePath: string,
): string | null {
    if (path.isAbsolute(filePath)) {
        return filePath;
    }

    if (!workspaceRoot) {
        return null;
    }

    return path.resolve(workspaceRoot, filePath);
}
