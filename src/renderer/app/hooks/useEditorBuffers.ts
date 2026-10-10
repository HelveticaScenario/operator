import { useCallback, useEffect, useRef, useState } from 'react';
import { v4 } from 'uuid';
import electronAPI from '../../electronAPI';
import type { EditorBuffer } from '../../types/editor';
import type { FSWriteResult } from '../../../shared/ipcTypes';
import {
    DEFAULT_PATCH,
    formatBufferLabel,
    getBufferId,
    normalizeFileName,
    readUnsavedBuffers,
    saveUnsavedBuffers,
    toAbsoluteWorkspacePath,
    withBufferContent,
    withDiskChange,
    withSavedContent,
} from '../buffers';

const writeErrorMessage = (result: FSWriteResult) =>
    ('error' in result && result.error) || 'Failed to save file';

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
    const [restoredBuffers] = useState(readUnsavedBuffers);
    const [buffers, setBuffers] = useState(restoredBuffers);

    const [activeBufferId, setActiveBufferId] = useState<string | undefined>(
        () =>
            restoredBuffers.length > 0
                ? getBufferId(restoredBuffers[0])
                : undefined,
    );

    const [renamingPath, setRenamingPath] = useState<string | null>(null);

    /** A save refused because the file changed on disk since it was read. */
    const [saveConflict, setSaveConflict] = useState<{
        bufferId: string;
        filePath: string;
    } | null>(null);

    const resolvePath = useCallback(
        (path: string) => toAbsoluteWorkspacePath(workspaceRoot, path),
        [workspaceRoot],
    );

    const activeBuffer = buffers.find((b) => getBufferId(b) === activeBufferId);
    const patchCode = activeBuffer?.content ?? DEFAULT_PATCH;

    useEffect(() => {
        saveUnsavedBuffers(buffers);
    }, [buffers]);

    const watchedPathsKey = JSON.stringify(
        buffers.flatMap((b) => (b.kind === 'file' ? [b.filePath] : [])).sort(),
    );
    useEffect(() => {
        electronAPI.filesystem
            .watchOpenFiles(JSON.parse(watchedPathsKey) as string[])
            .catch((error: unknown) =>
                console.error('Failed to watch open files:', error),
            );
    }, [watchedPathsKey]);

    useEffect(
        () =>
            electronAPI.filesystem.onFileChanged(({ filePath, content }) => {
                setBuffers((prev) =>
                    prev.map((b) =>
                        b.kind === 'file' && b.filePath === filePath
                            ? withDiskChange(b, content)
                            : b,
                    ),
                );
            }),
        [],
    );

    // Compare file buffers restored from storage against their files as they
    // are now. One whose baseline is unknown adopts the disk content as its
    // baseline; one with a baseline treats the disk content like an external
    // change, so edits made against an older file still conflict on save.
    useEffect(() => {
        for (const buffer of restoredBuffers) {
            if (buffer.kind !== 'file') {
                continue;
            }
            const { filePath, savedContent } = buffer;
            electronAPI.filesystem
                .readFile(filePath)
                .then((diskContent) => {
                    setBuffers((prev) =>
                        prev.map((b) => {
                            // A save or reload since the read makes it stale.
                            if (
                                b.kind !== 'file' ||
                                b.filePath !== filePath ||
                                b.savedContent !== savedContent
                            ) {
                                return b;
                            }
                            return b.savedContent === null
                                ? withSavedContent(b, diskContent)
                                : withDiskChange(b, diskContent);
                        }),
                    );
                })
                .catch(() => {
                    // A missing or unreadable file keeps the buffer dirty.
                });
        }
    }, [restoredBuffers]);

    const handlePatchChange = useCallback(
        (value: string) => {
            setBuffers((prev) =>
                prev.map((b) =>
                    getBufferId(b) === activeBufferId
                        ? { ...withBufferContent(b, value), isPreview: false }
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

            const absPath = toAbsoluteWorkspacePath(workspaceRoot, relPath);

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
                    savedContent: content,
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
                savedContent: content,
            };
            setBuffers((prev) => [...prev, newBuffer]);
            setActiveBufferId(absPath);
        },
        [buffers],
    );

    const createUntitledFile = useCallback(() => {
        setBuffers((prev) => {
            // Reserve every in-use untitled number from current state (avoiding
            // races). Files saved from an untitled buffer keep their
            // `untitled-N` id, so scan by id across all kinds — otherwise a new
            // untitled could re-mint a number a saved file still holds, giving
            // two buffers the same id.
            const currentUsed = new Set<number>();
            prev.forEach((b) => {
                const match = b.id.match(/^untitled-(\d+)$/);
                if (match) {
                    currentUsed.add(parseInt(match[1], 10));
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

            setActiveBufferId(nextId);
            return [...prev, newBuffer];
        });
    }, []);

    /**
     * Save a buffer to disk. Returns the buffer's id after the save (the
     * absolute file path — saving an untitled buffer changes its id), or
     * undefined when nothing was saved (buffer missing, dialog cancelled, or
     * a save conflict).
     *
     * A file that changed on disk since the buffer last read or wrote it is
     * not overwritten: the save is refused and `saveConflict` is set, unless
     * `overwrite` is passed.
     *
     * Edits can land while the async write is in flight, so the buffer is
     * compared against the snapshot that reached disk, not marked clean.
     */
    const saveFile = useCallback(
        async (targetId?: string, options?: { overwrite?: boolean }) => {
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
                const filePath = resolvePath(normalized);

                const result = await electronAPI.filesystem.writeFile(
                    filePath,
                    savedContent,
                );

                if (result.success) {
                    setBuffers((prev) => {
                        const source = prev.find(
                            (b) => getBufferId(b) === idToSave,
                        );
                        const existing = prev.find(
                            (b) =>
                                b.kind === 'file' &&
                                b.filePath === filePath &&
                                getBufferId(b) !== idToSave,
                        );
                        if (source && existing) {
                            // The chosen path is already open: fold the
                            // untitled buffer into the existing one so the
                            // path keeps a single buffer identity — two
                            // buffers sharing an id would make every id
                            // lookup act on whichever comes first.
                            return prev
                                .filter((b) => getBufferId(b) !== idToSave)
                                .map((b) =>
                                    b.kind === 'file' && b.filePath === filePath
                                        ? {
                                              ...withSavedContent(
                                                  {
                                                      ...b,
                                                      content: source.content,
                                                  },
                                                  savedContent,
                                              ),
                                              isPreview: false,
                                          }
                                        : b,
                                );
                        }
                        return prev.map((b) =>
                            getBufferId(b) === idToSave
                                ? {
                                      content: b.content,
                                      dirty: b.content !== savedContent,
                                      filePath,
                                      id: b.id,
                                      kind: 'file' as const,
                                      savedContent,
                                  }
                                : b,
                        );
                    });
                    if (idToSave === activeBufferId) {
                        setActiveBufferId(filePath);
                    }
                    await refreshFileTree();
                    onFileSaved?.(filePath);
                    return filePath;
                } else {
                    throw new Error(writeErrorMessage(result));
                }
            } else {
                const result = await electronAPI.filesystem.writeFile(
                    buffer.filePath,
                    savedContent,
                    options?.overwrite
                        ? undefined
                        : (buffer.savedContent ?? undefined),
                );

                if (result.success) {
                    setBuffers((prev) =>
                        prev.map((b) =>
                            getBufferId(b) === idToSave && b.kind === 'file'
                                ? withSavedContent(b, savedContent)
                                : b,
                        ),
                    );
                    onFileSaved?.(buffer.filePath);
                    return buffer.filePath;
                } else if ('conflict' in result) {
                    setSaveConflict({
                        bufferId: getBufferId(buffer),
                        filePath: buffer.filePath,
                    });
                    return undefined;
                } else {
                    throw new Error(writeErrorMessage(result));
                }
            }
        },
        [activeBufferId, buffers, refreshFileTree, onFileSaved, resolvePath],
    );

    const dismissSaveConflict = useCallback(() => setSaveConflict(null), []);

    /** Resolve a save conflict by writing the buffer over the file on disk. */
    const overwriteOnConflict = useCallback(
        async (bufferId: string) => {
            setSaveConflict(null);
            await saveFile(bufferId, { overwrite: true });
        },
        [saveFile],
    );

    /** Resolve a save conflict by replacing the buffer with the disk content. */
    const revertToDiskContent = useCallback(
        (bufferId: string, diskContent: string) => {
            setSaveConflict(null);
            setBuffers((prev) =>
                prev.map((b) =>
                    getBufferId(b) === bufferId && b.kind === 'file'
                        ? withSavedContent(
                              { ...b, content: diskContent },
                              diskContent,
                          )
                        : b,
                ),
            );
        },
        [],
    );

    const renameFile = useCallback(
        async (targetIdOrPath?: string) => {
            let filePath: string | undefined;

            const resolvedPath = targetIdOrPath
                ? resolvePath(targetIdOrPath)
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
        [activeBufferId, buffers, resolvePath],
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
                ? resolvePath(targetIdOrPath)
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
        [activeBufferId, buffers, refreshFileTree, resolvePath],
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
        dismissSaveConflict,
        formatFileLabel,
        handlePatchChange,
        handleRenameCommit,
        keepBuffer,
        openAbsoluteFile,
        openFile,
        overwriteOnConflict,
        patchCode,
        renameFile,
        renamingPath,
        revertToDiskContent,
        saveConflict,
        saveFile,
        setActiveBufferId,
        setBuffers,
        setRenamingPath,
    };
}
