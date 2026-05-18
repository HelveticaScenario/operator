import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
    Tree,
    type NodeApi,
    type NodeRendererProps,
    type TreeApi,
} from 'react-arborist';
import * as ContextMenu from '@radix-ui/react-context-menu';
import './FileExplorer.css';
import type { FileTreeEntry } from '../../shared/ipcTypes';
import type { EditorBuffer } from '../types/editor';
import { getBufferId } from '../app/buffers';
import electronAPI from '../electronAPI';
import { setContextKey } from '../keybindings/contextKeys';

const EXPANDED_STORAGE_KEY = 'modular_expanded_folders';
const ROW_HEIGHT = 22;
const INDENT = 14;

interface FileExplorerProps {
    workspaceRoot: string | null;
    fileTree: FileTreeEntry[];
    buffers: EditorBuffer[];
    activeBufferId?: string;
    runningBufferId: string | null;
    renamingPath: string | null;
    formatLabel: (buffer: EditorBuffer) => string;
    onSelectBuffer: (bufferId: string) => void;
    onOpenFile: (relPath: string, options?: { preview?: boolean }) => void;
    onCreateFile: () => void;
    onSaveFile: (id?: string) => void;
    onRenameFile: (id?: string) => void;
    onDeleteFile: (id?: string) => void;
    onCloseBuffer: (bufferId: string) => void;
    onSelectWorkspace: () => void;
    onRefreshTree: () => void;
    onRenameCommit: (path: string, newName: string) => void;
    onRenameCancel: () => void;
    onKeepBuffer: (bufferId: string) => void;
    /** Move a workspace file (drag-and-drop in the tree). */
    onMoveFile?: (sourcePath: string, destPath: string) => void | Promise<void>;
}

function basename(p: string): string {
    const parts = p.split(/[/\\]/);
    return parts[parts.length - 1] ?? p;
}

function dirname(p: string): string {
    const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return idx === -1 ? '' : p.slice(0, idx);
}

function joinPath(dir: string, name: string): string {
    if (!dir) return name;
    return `${dir}/${name}`;
}

function loadInitialOpenState(): Record<string, boolean> {
    try {
        const raw = window.localStorage.getItem(EXPANDED_STORAGE_KEY);
        if (raw) {
            const list = JSON.parse(raw) as string[];
            const map: Record<string, boolean> = {};
            for (const p of list) map[p] = true;
            return map;
        }
    } catch {}
    return {};
}

function persistOpenState(openState: Record<string, boolean>): void {
    try {
        const list = Object.entries(openState)
            .filter(([, v]) => v)
            .map(([k]) => k);
        window.localStorage.setItem(
            EXPANDED_STORAGE_KEY,
            JSON.stringify(list),
        );
    } catch {}
}

function BufferItem({
    buffer,
    isActive,
    isRunning,
    renamingPath,
    formatLabel,
    onSelectBuffer,
    onContextMenu,
    onCloseBuffer,
    onRenameCommit,
    onRenameCancel,
    onKeepBuffer,
}: {
    buffer: EditorBuffer;
    isActive: boolean;
    isRunning: boolean;
    renamingPath: string | null;
    formatLabel: (buffer: EditorBuffer) => string;
    onSelectBuffer: (id: string) => void;
    onContextMenu: (e: React.MouseEvent, id: string) => void;
    onCloseBuffer: (id: string) => void;
    onRenameCommit: (path: string, newName: string) => void;
    onRenameCancel: () => void;
    onKeepBuffer: (id: string) => void;
}) {
    const bufferId = getBufferId(buffer);
    const inputRef = useRef<HTMLInputElement>(null);
    const isRenaming =
        buffer.kind === 'file' && renamingPath === buffer.filePath;

    const formatLabelRef = useRef(formatLabel);
    formatLabelRef.current = formatLabel;
    const bufferRef = useRef(buffer);
    bufferRef.current = buffer;

    useEffect(() => {
        if (isRenaming && inputRef.current) {
            inputRef.current.focus();
            const name = formatLabelRef.current(bufferRef.current);
            const lastDotIndex = name.lastIndexOf('.');
            if (lastDotIndex !== -1) {
                inputRef.current.setSelectionRange(0, lastDotIndex);
            } else {
                inputRef.current.select();
            }
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isRenaming]);

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter') {
            e.stopPropagation();
            if (buffer.kind === 'file') {
                onRenameCommit(
                    buffer.filePath,
                    inputRef.current?.value ?? formatLabel(buffer),
                );
            }
        } else if (e.key === 'Escape') {
            e.stopPropagation();
            onRenameCancel();
        }
    };

    return (
        <li
            className={[
                'buffer-item',
                isActive ? 'active' : '',
                buffer.dirty ? 'dirty' : '',
                isRunning ? 'running' : '',
                buffer.isPreview ? 'preview' : '',
            ]
                .filter(Boolean)
                .join(' ')}
            onClick={() => !isRenaming && onSelectBuffer(bufferId)}
            onDoubleClick={() => !isRenaming && onKeepBuffer(bufferId)}
            onContextMenu={(e) => onContextMenu(e, bufferId)}
        >
            {isRenaming ? (
                <input
                    ref={inputRef}
                    type="text"
                    className="rename-input"
                    defaultValue={formatLabel(buffer)}
                    onKeyDown={handleKeyDown}
                    onBlur={onRenameCancel}
                    onClick={(e) => e.stopPropagation()}
                />
            ) : (
                <span className="file-name">{formatLabel(buffer)}</span>
            )}
            {!isRenaming && isRunning && (
                <span className="running-badge">▶</span>
            )}
            {!isRenaming && buffer.dirty && (
                <span className="dirty-dot">●</span>
            )}
            {!isRenaming && (
                <button
                    className="close-button"
                    onClick={(e) => {
                        e.stopPropagation();
                        onCloseBuffer(bufferId);
                    }}
                    title="Close"
                >
                    ×
                </button>
            )}
        </li>
    );
}

/**
 * Custom row renderer for arborist. Wraps each row in a Radix
 * context menu and exposes inline rename via `node.edit()`.
 */
function TreeRow({
    node,
    style,
    dragHandle,
    tree,
    onRevealInFinder,
    onDeleteEntry,
    onCreateFileInDir,
    onCreateFolderInDir,
}: NodeRendererProps<FileTreeEntry> & {
    tree: TreeApi<FileTreeEntry>;
    onRevealInFinder: (path: string) => void;
    onDeleteEntry: (path: string) => void;
    onCreateFileInDir: (parentDir: string) => void;
    onCreateFolderInDir: (parentDir: string) => void;
}) {
    const entry = node.data;
    const isDir = entry.type === 'directory';
    const isWav = entry.fileType === 'wav';
    const icon = isDir ? (node.isOpen ? '📂' : '📁') : isWav ? '🔊' : '📄';

    const parentDir = isDir ? entry.path : dirname(entry.path);

    return (
        <ContextMenu.Root>
            <ContextMenu.Trigger asChild>
                <div
                    ref={dragHandle}
                    style={style}
                    className={[
                        'arborist-row',
                        isDir ? 'arborist-folder' : 'arborist-file',
                        isWav ? 'arborist-file-wav' : '',
                        node.isSelected ? 'selected' : '',
                        node.isFocused ? 'focused' : '',
                        node.willReceiveDrop ? 'drop-target' : '',
                    ]
                        .filter(Boolean)
                        .join(' ')}
                    onClick={(e) => {
                        // Avoid hijacking the toggle chevron click below.
                        if ((e.target as HTMLElement).dataset.toggle) return;
                        node.handleClick(e);
                    }}
                >
                    <span
                        className="arborist-toggle"
                        data-toggle="1"
                        onClick={(e) => {
                            e.stopPropagation();
                            if (isDir) node.toggle();
                        }}
                    >
                        {isDir ? (node.isOpen ? '▾' : '▸') : ''}
                    </span>
                    <span className="arborist-icon">{icon}</span>
                    {node.isEditing ? (
                        <input
                            className="rename-input"
                            autoFocus
                            defaultValue={entry.name}
                            onClick={(e) => e.stopPropagation()}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter') {
                                    e.stopPropagation();
                                    node.submit(
                                        (e.target as HTMLInputElement).value,
                                    );
                                } else if (e.key === 'Escape') {
                                    e.stopPropagation();
                                    node.reset();
                                }
                            }}
                            onBlur={() => node.reset()}
                        />
                    ) : (
                        <span className="arborist-name">{entry.name}</span>
                    )}
                </div>
            </ContextMenu.Trigger>
            <ContextMenu.Portal>
                <ContextMenu.Content className="context-menu-content">
                    <ContextMenu.Item
                        className="context-menu-item"
                        onSelect={() => onCreateFileInDir(parentDir)}
                    >
                        New File
                    </ContextMenu.Item>
                    <ContextMenu.Item
                        className="context-menu-item"
                        onSelect={() => onCreateFolderInDir(parentDir)}
                    >
                        New Folder
                    </ContextMenu.Item>
                    <ContextMenu.Separator className="context-menu-separator" />
                    <ContextMenu.Item
                        className="context-menu-item"
                        disabled={isWav}
                        onSelect={() => {
                            // Defer to allow Radix to close the menu first.
                            setTimeout(() => {
                                tree.select(node);
                                void node.edit();
                            }, 0);
                        }}
                    >
                        Rename
                    </ContextMenu.Item>
                    <ContextMenu.Item
                        className="context-menu-item danger"
                        onSelect={() => onDeleteEntry(entry.path)}
                    >
                        Delete
                    </ContextMenu.Item>
                    <ContextMenu.Separator className="context-menu-separator" />
                    <ContextMenu.Item
                        className="context-menu-item"
                        onSelect={() => onRevealInFinder(entry.path)}
                    >
                        Reveal in Finder/Explorer
                    </ContextMenu.Item>
                </ContextMenu.Content>
            </ContextMenu.Portal>
        </ContextMenu.Root>
    );
}

function WorkspaceTree({
    fileTree,
    onOpenFile,
    onMoveFile,
    onRenameCommit,
    onDeleteFile,
    onRefreshTree,
}: {
    fileTree: FileTreeEntry[];
    onOpenFile: (relPath: string, options?: { preview?: boolean }) => void;
    onMoveFile?: (sourcePath: string, destPath: string) => void | Promise<void>;
    onRenameCommit: (path: string, newName: string) => void;
    onDeleteFile: (id?: string) => void;
    onRefreshTree: () => void;
}) {
    const containerRef = useRef<HTMLDivElement>(null);
    const treeRef = useRef<TreeApi<FileTreeEntry> | null>(null);
    const [size, setSize] = useState({
        width: 0,
        height: 0,
    });

    useEffect(() => {
        const el = containerRef.current;
        if (!el) return;
        const update = () => {
            setSize({ width: el.clientWidth, height: el.clientHeight });
        };
        update();
        const ro = new ResizeObserver(update);
        ro.observe(el);
        return () => ro.disconnect();
    }, []);

    const initialOpenState = useMemo(() => loadInitialOpenState(), []);

    const handleToggle = useCallback(() => {
        const state = treeRef.current?.openState ?? {};
        persistOpenState(state as Record<string, boolean>);
    }, []);

    const handleActivate = useCallback(
        (node: NodeApi<FileTreeEntry>) => {
            const entry = node.data;
            if (entry.type === 'directory') return;
            if (entry.fileType === 'wav') return;
            onOpenFile(entry.path, { preview: false });
        },
        [onOpenFile],
    );

    const handleClickNode = useCallback(
        (node: NodeApi<FileTreeEntry>) => {
            const entry = node.data;
            if (entry.type === 'directory') return;
            if (entry.fileType === 'wav') return;
            onOpenFile(entry.path, { preview: true });
        },
        [onOpenFile],
    );

    const handleFocus = useCallback(
        (node: NodeApi<FileTreeEntry>) => {
            // Preview-open on focus (keyboard navigation).
            handleClickNode(node);
        },
        [handleClickNode],
    );

    const handleRename = useCallback(
        ({ id, name }: { id: string; name: string }) => {
            onRenameCommit(id, name);
        },
        [onRenameCommit],
    );

    const handleMove = useCallback(
        ({
            dragIds,
            parentNode,
        }: {
            dragIds: string[];
            parentId: string | null;
            parentNode: NodeApi<FileTreeEntry> | null;
            index: number;
        }) => {
            if (!onMoveFile) return;
            const parentDir = parentNode ? parentNode.data.path : '';
            for (const sourcePath of dragIds) {
                const destPath = joinPath(parentDir, basename(sourcePath));
                if (destPath === sourcePath) continue;
                void onMoveFile(sourcePath, destPath);
            }
        },
        [onMoveFile],
    );

    const handleRevealInFinder = useCallback(async (path: string) => {
        await electronAPI.filesystem.revealInFinder(path);
    }, []);

    const handleDeleteEntry = useCallback(
        (path: string) => {
            onDeleteFile(path);
        },
        [onDeleteFile],
    );

    const handleCreateFileInDir = useCallback(
        async (parentDir: string) => {
            const name = await electronAPI.filesystem.showInputDialog(
                'New File',
                'untitled.mjs',
            );
            if (!name) return;
            const relPath = joinPath(parentDir, name);
            const result = await electronAPI.filesystem.writeFile(relPath, '');
            if (result.success) {
                onRefreshTree();
                onOpenFile(relPath, { preview: false });
            }
        },
        [onOpenFile, onRefreshTree],
    );

    const handleCreateFolderInDir = useCallback(
        async (parentDir: string) => {
            const name = await electronAPI.filesystem.showInputDialog(
                'New Folder',
                'untitled-folder',
            );
            if (!name) return;
            const relPath = joinPath(parentDir, name);
            const result = await electronAPI.filesystem.createFolder(relPath);
            if (result.success) {
                onRefreshTree();
            }
        },
        [onRefreshTree],
    );

    return (
        <div ref={containerRef} className="arborist-container">
            {fileTree.length === 0 ? (
                <div className="empty-message">No files found</div>
            ) : (
                size.height > 0 && (
                    <Tree
                        ref={treeRef}
                        data={fileTree}
                        idAccessor="path"
                        childrenAccessor={(d) => d.children ?? null}
                        openByDefault={false}
                        initialOpenState={initialOpenState}
                        width={size.width}
                        height={size.height}
                        rowHeight={ROW_HEIGHT}
                        indent={INDENT}
                        overscanCount={8}
                        onToggle={handleToggle}
                        onActivate={handleActivate}
                        onFocus={handleFocus}
                        onRename={handleRename}
                        onMove={handleMove}
                        disableEdit={(entry) => entry.fileType === 'wav'}
                        disableDrop={({ parentNode }) =>
                            parentNode !== null &&
                            parentNode.data.type !== 'directory'
                        }
                    >
                        {(props) => (
                            <TreeRow
                                {...props}
                                tree={treeRef.current as TreeApi<FileTreeEntry>}
                                onRevealInFinder={handleRevealInFinder}
                                onDeleteEntry={handleDeleteEntry}
                                onCreateFileInDir={handleCreateFileInDir}
                                onCreateFolderInDir={handleCreateFolderInDir}
                            />
                        )}
                    </Tree>
                )
            )}
        </div>
    );
}

export function FileExplorer({
    workspaceRoot,
    fileTree,
    buffers,
    activeBufferId,
    runningBufferId,
    renamingPath,
    formatLabel,
    onSelectBuffer,
    onOpenFile,
    onCreateFile,
    onSaveFile: _onSaveFile,
    onRenameFile: _onRenameFile,
    onDeleteFile,
    onCloseBuffer,
    onSelectWorkspace: _onSelectWorkspace,
    onRefreshTree,
    onRenameCommit,
    onRenameCancel,
    onKeepBuffer,
    onMoveFile,
}: FileExplorerProps) {
    const handleBufferContextMenu = (
        e: React.MouseEvent,
        bufferId: string,
    ) => {
        e.preventDefault();
        const buffer = buffers.find((b) => getBufferId(b) === bufferId);

        let contextType: 'file' | 'untitled' | 'unknown' = 'unknown';
        if (buffer?.kind === 'file') {
            contextType = 'file';
        } else if (buffer?.kind === 'untitled') {
            contextType = 'untitled';
        }

        void electronAPI.showContextMenu({
            type: contextType,
            path: buffer?.kind === 'file' ? buffer.filePath : undefined,
            bufferId,
            isOpenBuffer: true,
            isWorkspaceFile: false,
            x: e.clientX,
            y: e.clientY,
        });
    };

    return (
        <div
            className="file-explorer"
            onFocus={() => setContextKey('fileExplorerFocused', true)}
            onBlur={(e) => {
                if (!e.currentTarget.contains(e.relatedTarget as Node)) {
                    setContextKey('fileExplorerFocused', false);
                }
            }}
        >
            <div className="file-sections">
                {/* Open Editors Section */}
                <div className="section">
                    <div className="section-header">
                        <span>Open Editors</span>
                        <button
                            onClick={onCreateFile}
                            title="New untitled file"
                            className="section-action"
                        >
                            +
                        </button>
                    </div>
                    <div className="file-list">
                        {buffers.length === 0 ? (
                            <div className="empty-message">No open files</div>
                        ) : (
                            <ul>
                                {buffers.map((buffer) => {
                                    const bufferId = getBufferId(buffer);
                                    return (
                                        <BufferItem
                                            key={bufferId}
                                            buffer={buffer}
                                            isActive={
                                                bufferId === activeBufferId
                                            }
                                            isRunning={
                                                bufferId === runningBufferId
                                            }
                                            renamingPath={renamingPath}
                                            formatLabel={formatLabel}
                                            onSelectBuffer={onSelectBuffer}
                                            onContextMenu={
                                                handleBufferContextMenu
                                            }
                                            onCloseBuffer={onCloseBuffer}
                                            onRenameCommit={onRenameCommit}
                                            onRenameCancel={onRenameCancel}
                                            onKeepBuffer={onKeepBuffer}
                                        />
                                    );
                                })}
                            </ul>
                        )}
                    </div>
                </div>

                {/* Workspace Files Tree */}
                {workspaceRoot && (
                    <div className="section section-workspace">
                        <div className="section-header">
                            <span>Workspace Files</span>
                            <button
                                onClick={onRefreshTree}
                                title="Refresh file tree"
                                className="section-action"
                            >
                                ↻
                            </button>
                        </div>
                        <WorkspaceTree
                            fileTree={fileTree}
                            onOpenFile={onOpenFile}
                            onMoveFile={onMoveFile}
                            onRenameCommit={onRenameCommit}
                            onDeleteFile={onDeleteFile}
                            onRefreshTree={onRefreshTree}
                        />
                    </div>
                )}
            </div>
        </div>
    );
}
