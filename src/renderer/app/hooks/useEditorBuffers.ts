import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { v4 } from 'uuid';
import electronAPI from '../../electronAPI';
import type { EditorBuffer } from '../../types/editor';
import {
    DEFAULT_PATCH,
    formatBufferLabel,
    getBufferId,
    normalizeFileName,
    readUnsavedBuffers,
    saveUnsavedBuffers,
} from '../buffers';

interface UseEditorBuffersParams {
    workspaceRoot: string | null;
    refreshFileTree: () => Promise<void>;
    /** Called with the absolute path after a buffer is successfully saved. */
    onFileSaved?: (filePath: string) => void;
}

export function useEditorBuffers({
    workspaceRoot,
    refreshFileTree,
    onFileSaved,
}: UseEditorBuffersParams) {
    const [buffers, setBuffers] = useState<EditorBuffer[]>(() => {
        const saved = readUnsavedBuffers();
        return saved;
    });

    const [activeBufferId, setActiveBufferId] = useState<string | undefined>(
        () => {
            const saved = readUnsavedBuffers();
            return saved.length > 0 ? getBufferId(saved[0]) : undefined;
        },
    );

    const usedUntitledNumbers = useMemo(() => {
        const used = new Set<number>();
        buffers.forEach((b) => {
            if (b.kind === 'untitled') {
                const match = b.id.match(/^untitled-(\d+)$/);
                if (match) {
                    used.add(parseInt(match[1], 10));
                }
            }
        });
        return used;
    }, [buffers]);
    const usedUntitledNumbersRef = useRef(usedUntitledNumbers);
    useEffect(() => {
        usedUntitledNumbersRef.current = usedUntitledNumbers;
    });

    const [renamingPath, setRenamingPath] = useState<string | null>(null);

    // File buffers are identified by absolute path everywhere (getBufferId
    // returns filePath). IPC surfaces that hand back workspace-relative paths
    // (the save dialog, the file tree, context menus) must be resolved here
    // before the path is stored or compared.
    const resolveWorkspacePath = useCallback(
        (path: string) => {
            if (
                workspaceRoot &&
                !path.startsWith('/') &&
                !path.match(/^[a-zA-Z]:/)
            ) {
                return `${workspaceRoot}/${path}`;
            }
            return path;
        },
        [workspaceRoot],
    );

    const activeBuffer = buffers.find((b) => getBufferId(b) === activeBufferId);
    const patchCode = activeBuffer?.content ?? DEFAULT_PATCH;

    useEffect(() => {
        saveUnsavedBuffers(buffers);
    }, [buffers]);

    const handlePatchChange = useCallback(
        (value: string) => {
            setBuffers((prev) =>
                prev.map((b) =>
                    getBufferId(b) === activeBufferId
                        ? {
                              ...b,
                              content: value,
                              dirty: true,
                              isPreview: false,
                          }
                        : b,
                ),
            );
        },
        [activeBufferId],
    );

    const openFile = useCallback(
        async (relPath: string, options?: { preview?: boolean }) => {
            if (!workspaceRoot) {
                throw new Error('No workspace open');
            }

            const absPath = `${workspaceRoot}/${relPath}`;

            const existing = buffers.find(
                (b) => b.kind === 'file' && b.filePath === absPath,
            );

            if (existing) {
                if (options?.preview === false && existing.isPreview) {
                    setBuffers((prev) =>
                        prev.map((b) =>
                            getBufferId(b) === getBufferId(existing)
                                ? { ...b, isPreview: false }
                                : b,
                        ),
                    );
                }
                setActiveBufferId(getBufferId(existing));
                return;
            }

            const content = await electronAPI.filesystem.readFile(absPath);

            setBuffers((prev) => {
                const nextBuffers = [...prev];
                const existingPreviewIndex = nextBuffers.findIndex(
                    (b) => b.isPreview,
                );

                if (options?.preview && existingPreviewIndex !== -1) {
                    const previewBuffer = nextBuffers[existingPreviewIndex];
                    if (!previewBuffer.dirty) {
                        nextBuffers.splice(existingPreviewIndex, 1);
                    }
                }

                const newBuffer: EditorBuffer = {
                    content,
                    dirty: false,
                    filePath: absPath,
                    id: v4(),
                    isPreview: options?.preview ?? false,
                    kind: 'file',
                };
                return [...nextBuffers, newBuffer];
            });
            setActiveBufferId(absPath);
        },
        [buffers, workspaceRoot],
    );

    // Open a file by absolute path, bypassing the workspace-relative path
    // join used by `openFile`. Used for files outside the workspace such as
    // the user keybindings.json in userData.
    const openAbsoluteFile = useCallback(
        async (absPath: string) => {
            const existing = buffers.find(
                (b) => b.kind === 'file' && b.filePath === absPath,
            );
            if (existing) {
                setActiveBufferId(getBufferId(existing));
                return;
            }
            const content = await electronAPI.filesystem.readFile(absPath);
            const newBuffer: EditorBuffer = {
                content,
                dirty: false,
                filePath: absPath,
                id: v4(),
                isPreview: false,
                kind: 'file',
            };
            setBuffers((prev) => [...prev, newBuffer]);
            setActiveBufferId(absPath);
        },
        [buffers],
    );

    const createUntitledFile = useCallback(() => {
        setBuffers((prev) => {
            // Derive next ID from current state to avoid race conditions
            const currentUsed = new Set<number>();
            prev.forEach((b) => {
                if (b.kind === 'untitled') {
                    const match = b.id.match(/^untitled-(\d+)$/);
                    if (match) {
                        currentUsed.add(parseInt(match[1], 10));
                    }
                }
            });

            let nextIdNum = 1;
            while (currentUsed.has(nextIdNum)) {
                nextIdNum++;
            }

            const nextId = `untitled-${nextIdNum}`;
            const newBuffer: EditorBuffer = {
                content: DEFAULT_PATCH,
                dirty: false,
                id: nextId,
                kind: 'untitled',
            };

            // Update ref for useMemo dependency
            const next = new Set(currentUsed);
            next.add(nextIdNum);
            usedUntitledNumbersRef.current = next;

            setActiveBufferId(nextId);
            return [...prev, newBuffer];
        });
    }, []);

    /**
     * Save a buffer to disk. Returns the buffer's id after the save (the
     * absolute file path — saving an untitled buffer changes its id), or
     * undefined when nothing was saved (buffer missing, dialog cancelled).
     *
     * Edits can land while the async write is in flight, so the dirty flag is
     * only cleared on buffers whose content still equals the snapshot that
     * reached disk.
     */
    const saveFile = useCallback(
        async (targetId?: string) => {
            const idToSave = targetId || activeBufferId;
            const buffer = buffers.find((b) => getBufferId(b) === idToSave);
            if (!buffer) {
                return undefined;
            }
            const savedContent = buffer.content;

            if (buffer.kind === 'untitled') {
                const input =
                    await electronAPI.filesystem.showSaveDialog('untitled.mjs');
                if (!input) {
                    return undefined;
                }

                const normalized = normalizeFileName(input);
                if (!normalized) {
                    return undefined;
                }

                // The save dialog only ever returns workspace-relative paths.
                const filePath = resolveWorkspacePath(normalized);

                const result = await electronAPI.filesystem.writeFile(
                    filePath,
                    savedContent,
                );

                if (result.success) {
                    setBuffers((prev) =>
                        prev.map((b) =>
                            getBufferId(b) === idToSave
                                ? {
                                      content: b.content,
                                      dirty: b.content !== savedContent,
                                      filePath,
                                      id: b.id,
                                      kind: 'file' as const,
                                  }
                                : b,
                        ),
                    );
                    if (idToSave === activeBufferId) {
                        setActiveBufferId(filePath);
                    }
                    await refreshFileTree();
                    onFileSaved?.(filePath);
                    return filePath;
                } else {
                    throw new Error(result.error || 'Failed to save file');
                }
            } else {
                const result = await electronAPI.filesystem.writeFile(
                    buffer.filePath,
                    savedContent,
                );

                if (result.success) {
                    setBuffers((prev) =>
                        prev.map((b) =>
                            getBufferId(b) === idToSave &&
                            b.content === savedContent
                                ? { ...b, dirty: false }
                                : b,
                        ),
                    );
                    onFileSaved?.(buffer.filePath);
                    return buffer.filePath;
                } else {
                    throw new Error(result.error || 'Failed to save file');
                }
            }
        },
        [
            activeBufferId,
            buffers,
            refreshFileTree,
            onFileSaved,
            resolveWorkspacePath,
        ],
    );

    const renameFile = useCallback(
        async (targetIdOrPath?: string) => {
            let filePath: string | undefined;

            const resolvedPath = targetIdOrPath
                ? resolveWorkspacePath(targetIdOrPath)
                : targetIdOrPath;

            const buffer =
                buffers.find((b) => getBufferId(b) === targetIdOrPath) ||
                buffers.find(
                    (b) => b.kind === 'file' && b.filePath === resolvedPath,
                );

            if (buffer && buffer.kind === 'file') {
                ({ filePath } = buffer);
            } else if (resolvedPath && typeof resolvedPath === 'string') {
                filePath = resolvedPath;
            } else if (activeBufferId) {
                const active = buffers.find(
                    (b) => getBufferId(b) === activeBufferId,
                );
                if (active && active.kind === 'file') {
                    ({ filePath } = active);
                }
            }

            if (!filePath) {
                return;
            }
            setRenamingPath(filePath);
        },
        [activeBufferId, buffers, resolveWorkspacePath],
    );

    const handleRenameCommit = useCallback(
        async (oldPath: string, newName: string) => {
            setRenamingPath(null);
            if (!newName) {
                return;
            }

            const currentFileName = oldPath.split(/[/\\]/).pop();
            if (newName === currentFileName) {
                return;
            }

            const normalized = normalizeFileName(newName);

            const separator = oldPath.includes('\\') ? '\\' : '/';
            const lastSepIndex = oldPath.lastIndexOf(separator);
            let newPath = normalized;
            if (lastSepIndex !== -1) {
                const dir = oldPath.substring(0, lastSepIndex);
                newPath = `${dir}${separator}${normalized}`;
            }

            if (!newPath || newPath === oldPath) {
                return;
            }

            const result = await electronAPI.filesystem.renameFile(
                oldPath,
                newPath,
            );

            if (result.success) {
                const wasActive = activeBufferId === oldPath;

                setBuffers((prev) =>
                    prev.map((b) =>
                        b.kind === 'file' && b.filePath === oldPath
                            ? { ...b, filePath: newPath }
                            : b,
                    ),
                );

                if (wasActive) {
                    setActiveBufferId(newPath);
                }

                await refreshFileTree();
            } else {
                throw new Error(result.error || 'Failed to rename file');
            }
        },
        [activeBufferId, refreshFileTree],
    );

    const deleteFile = useCallback(
        async (targetIdOrPath?: string) => {
            let filePath: string | undefined;
            let bufferId: string | undefined;

            const resolvedPath = targetIdOrPath
                ? resolveWorkspacePath(targetIdOrPath)
                : targetIdOrPath;

            const buffer =
                buffers.find((b) => getBufferId(b) === targetIdOrPath) ||
                buffers.find(
                    (b) => b.kind === 'file' && b.filePath === resolvedPath,
                );

            if (buffer && buffer.kind === 'file') {
                ({ filePath } = buffer);
                bufferId = getBufferId(buffer);
            } else if (resolvedPath && typeof resolvedPath === 'string') {
                filePath = resolvedPath;
            } else if (activeBufferId) {
                const active = buffers.find(
                    (b) => getBufferId(b) === activeBufferId,
                );
                if (active && active.kind === 'file') {
                    ({ filePath } = active);
                    bufferId = getBufferId(active);
                }
            }

            if (!filePath) {
                return;
            }

            if (!window.confirm(`Delete ${filePath}?`)) {
                return;
            }

            const result = await electronAPI.filesystem.deleteFile(filePath);

            if (result.success) {
                // Use functional setState to avoid stale closure issues
                let activeIsDeleted = false;
                let remaining: typeof buffers = [];

                setBuffers((prev) => {
                    const currentActiveBuffer = prev.find(
                        (b) => getBufferId(b) === activeBufferId,
                    );
                    activeIsDeleted =
                        activeBufferId !== undefined &&
                        ((bufferId !== undefined &&
                            activeBufferId === bufferId) ||
                            (currentActiveBuffer?.kind === 'file' &&
                                currentActiveBuffer.filePath === filePath));

                    remaining = prev.filter(
                        (b) => !(b.kind === 'file' && b.filePath === filePath),
                    );
                    return remaining;
                });

                if (activeIsDeleted) {
                    if (remaining.length > 0) {
                        setActiveBufferId(getBufferId(remaining[0]));
                    } else {
                        setActiveBufferId(undefined);
                    }
                }

                await refreshFileTree();
            } else {
                throw new Error(result.error || 'Failed to delete file');
            }
        },
        [activeBufferId, buffers, refreshFileTree, resolveWorkspacePath],
    );

    // Mirror of activeBufferId for deferred callbacks that run after state
    // updates (e.g. a save that re-identified the buffer) have flushed.
    const activeBufferIdRef = useRef(activeBufferId);
    useEffect(() => {
        activeBufferIdRef.current = activeBufferId;
    });

    const performCloseBuffer = useCallback((bufferId: string) => {
        setTimeout(() => {
            // Read the active id when the deferred update runs, so a
            // just-completed save that changed either id is observed.
            const currentActiveId = activeBufferIdRef.current;
            setBuffers((prev) => {
                const buffer = prev.find((b) => getBufferId(b) === bufferId);
                if (!buffer) {
                    return prev;
                }

                const remaining = prev.filter(
                    (b) => getBufferId(b) !== bufferId,
                );

                // Update active buffer if we're closing the active one
                if (currentActiveId === bufferId) {
                    const idx = prev.findIndex(
                        (b) => getBufferId(b) === bufferId,
                    );
                    if (remaining.length > 0) {
                        // Select the buffer that was immediately after the closed one,
                        // Or the last one if we closed the tail.
                        const nextIdx = Math.min(idx, remaining.length - 1);
                        setActiveBufferId(getBufferId(remaining[nextIdx]));
                    } else {
                        setActiveBufferId(undefined);
                    }
                }

                return remaining;
            });
        }, 50);
    }, []);

    const closeBuffer = useCallback(
        async (bufferId: string) => {
            const buffer = buffers.find((b) => getBufferId(b) === bufferId);
            if (!buffer) {
                return;
            }

            if (buffer.dirty) {
                const response = await electronAPI.showUnsavedChangesDialog(
                    formatBufferLabel(buffer),
                );

                if (response === 2) {
                    return;
                } else if (response === 0) {
                    try {
                        // Saving an untitled buffer changes its id to the
                        // chosen file path; close under the post-save id. A
                        // cancelled save dialog aborts the close so the
                        // unsaved content is not discarded.
                        const savedId = await saveFile(bufferId);
                        if (savedId === undefined) {
                            return;
                        }
                        performCloseBuffer(savedId);
                    } catch (error) {
                        // A failed save aborts the close: the content never
                        // reached disk, so the buffer must stay open.
                        console.error('Error saving file:', error);
                    }
                } else {
                    performCloseBuffer(bufferId);
                }
            } else {
                performCloseBuffer(bufferId);
            }
        },
        [buffers, saveFile, performCloseBuffer],
    );

    const keepBuffer = useCallback((bufferId: string) => {
        setBuffers((prev) =>
            prev.map((b) =>
                getBufferId(b) === bufferId ? { ...b, isPreview: false } : b,
            ),
        );
    }, []);

    const formatFileLabel = useCallback(
        (buffer: EditorBuffer) => formatBufferLabel(buffer),
        [],
    );

    return {
        activeBufferId,
        buffers,
        closeBuffer,
        createUntitledFile,
        deleteFile,
        formatFileLabel,
        handlePatchChange,
        handleRenameCommit,
        keepBuffer,
        openAbsoluteFile,
        openFile,
        patchCode,
        renameFile,
        renamingPath,
        saveFile,
        setActiveBufferId,
        setBuffers,
        setRenamingPath,
    };
}
