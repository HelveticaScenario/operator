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
}

export function useEditorBuffers({
    workspaceRoot,
    refreshFileTree,
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

    const saveFile = useCallback(
        async (targetId?: string) => {
            const idToSave = targetId || activeBufferId;
            const buffer = buffers.find((b) => getBufferId(b) === idToSave);
            if (!buffer) {
                return;
            }

            if (buffer.kind === 'untitled') {
                const input =
                    await electronAPI.filesystem.showSaveDialog('untitled.mjs');
                if (!input) {
                    return;
                }

                const normalized = normalizeFileName(input);
                if (!normalized) {
                    return;
                }

                const result = await electronAPI.filesystem.writeFile(
                    normalized,
                    buffer.content,
                );

                if (result.success) {
                    setBuffers((prev) =>
                        prev.map((b) =>
                            getBufferId(b) === idToSave
                                ? {
                                      content: buffer.content,
                                      dirty: false,
                                      filePath: normalized,
                                      id: b.id,
                                      kind: 'file' as const,
                                  }
                                : b,
                        ),
                    );
                    if (idToSave === activeBufferId) {
                        setActiveBufferId(normalized);
                    }
                    await refreshFileTree();
                } else {
                    throw new Error(result.error || 'Failed to save file');
                }
            } else {
                const result = await electronAPI.filesystem.writeFile(
                    buffer.filePath,
                    buffer.content,
                );

                if (result.success) {
                    setBuffers((prev) =>
                        prev.map((b) =>
                            getBufferId(b) === idToSave
                                ? { ...b, dirty: false }
                                : b,
                        ),
                    );
                } else {
                    throw new Error(result.error || 'Failed to save file');
                }
            }
        },
        [activeBufferId, buffers, refreshFileTree],
    );

    const renameFile = useCallback(
        async (targetIdOrPath?: string) => {
            let filePath: string | undefined;

            let resolvedPath = targetIdOrPath;
            if (
                targetIdOrPath &&
                workspaceRoot &&
                !targetIdOrPath.startsWith('/') &&
                !targetIdOrPath.match(/^[a-zA-Z]:/)
            ) {
                resolvedPath = `${workspaceRoot}/${targetIdOrPath}`;
            }

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
        [activeBufferId, buffers, workspaceRoot],
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

            let resolvedPath = targetIdOrPath;
            if (
                targetIdOrPath &&
                workspaceRoot &&
                !targetIdOrPath.startsWith('/') &&
                !targetIdOrPath.match(/^[a-zA-Z]:/)
            ) {
                resolvedPath = `${workspaceRoot}/${targetIdOrPath}`;
            }

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
        [activeBufferId, buffers, refreshFileTree, workspaceRoot],
    );

    const performCloseBuffer = useCallback(
        (bufferId: string) => {
            // Capture current activeBufferId to avoid stale closure
            const currentActiveId = activeBufferId;
            setTimeout(() => {
                setBuffers((prev) => {
                    const buffer = prev.find(
                        (b) => getBufferId(b) === bufferId,
                    );
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
        },
        [activeBufferId],
    );

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
                        await saveFile(bufferId);
                        performCloseBuffer(bufferId);
                    } catch (error) {
                        console.error('Error saving file:', error);
                        performCloseBuffer(bufferId);
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
