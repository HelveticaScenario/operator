import { useCallback, useEffect, useRef, useState } from 'react';
import { MonacoPatchEditor as PatchEditor } from './components/MonacoPatchEditor';
import { AudioControls } from './components/AudioControls';
import { TransportDisplay } from './components/TransportDisplay';
import { ErrorDisplay } from './components/ErrorDisplay';
import { Settings } from './components/Settings';
import { EngineHealth } from './components/EngineHealth';
import type { UpdateNotificationState } from './components/UpdateNotification';
import { UpdateNotification } from './components/UpdateNotification';
import './App.css';
// Import type { editor } from 'monaco-editor';
import { editor } from 'monaco-editor';
import { getErrorMessage } from './utils/errorUtils';
import { FileExplorer } from './components/FileExplorer';
import { Sidebar } from './components/Sidebar';
import { ControlPanel } from './components/ControlPanel';
import electronAPI from './electronAPI';
import type { ValidationError } from '@modular/core';
import type { QueuedTrigger } from '@modular/core';
import type {
    FileTreeEntry,
    SourceLocationInfo,
    TransportSnapshot,
    UpdateAvailableInfo,
} from '../shared/ipcTypes';
import type { SliderDefinition } from '../shared/dsl/sliderTypes';
import type { EditorBuffer } from './types/editor';
import { findSliderValueSpan } from './dsl/sliderSourceEdit';
import type { ScopeView } from './types/editor';
import { setActiveInterpolationResolutions } from '../shared/dsl/spanTypes';
import {
    drawOscilloscope,
    readScopeColors,
    scopeBufferKeyFromChannel,
    scopeBufferKeyToString,
} from './app/oscilloscope';
import { useEditorBuffers } from './app/hooks/useEditorBuffers';

/**
 * Transform validation errors to use source line numbers instead of module IDs
 * for auto-generated modules (where the ID is meaningless to the user).
 */
function transformErrorsWithSourceLocations(
    errors: ValidationError[],
    sourceLocationMap?: Record<string, SourceLocationInfo>,
): ValidationError[] {
    if (!sourceLocationMap) {
        return errors;
    }

    return errors.map((err) => {
        // The location field contains module ID like "sine-1" or user's explicit ID
        if (!err.location) {
            return err;
        }

        // Parse the location - it's either:
        // - "'myModule'" for explicit IDs (from format_module_location in Rust)
        // - "moduleName(...)" for auto-generated IDs
        const explicitIdMatch = err.location.match(/^'([^']+)'$/);
        if (explicitIdMatch) {
            // User explicitly named this module - keep showing the ID
            return err;
        }

        // For auto-generated module locations like "sine(...)",
        // Try to find source line from the map
        // The moduleType(...) format is produced by Rust, but we need the actual moduleId
        // To look up in the map. Let's check all entries in the map.
        for (const [moduleId, loc] of Object.entries(sourceLocationMap)) {
            if (
                !loc.idIsExplicit &&
                err.location.includes(moduleId.split('-')[0])
            ) {
                // Found a match - replace location with line number
                return {
                    ...err,
                    location: `line ${loc.line}`,
                };
            }
        }

        return err;
    });
}

function App() {
    // Workspace & filesystem
    const [workspaceRoot, setWorkspaceRoot] = useState<string | null>(null);
    const [fileTree, setFileTree] = useState<FileTreeEntry[]>([]);

    const refreshFileTree = useCallback(async () => {
        try {
            const tree = await electronAPI.filesystem.listFiles();
            setFileTree(tree);
        } catch (error) {
            console.error('Failed to refresh file tree:', error);
        }
    }, []);

    const {
        buffers,
        setBuffers,
        activeBufferId,
        setActiveBufferId,
        patchCode,
        handlePatchChange,
        openFile,
        createUntitledFile,
        saveFile,
        renameFile,
        deleteFile,
        closeBuffer,
        keepBuffer,
        renamingPath,
        setRenamingPath,
        handleRenameCommit,
        formatFileLabel,
    } = useEditorBuffers({ refreshFileTree, workspaceRoot });

    // Audio state
    const [isClockRunning, setIsClockRunning] = useState(true);
    const [isRecording, setIsRecording] = useState(false);
    const [isSettingsOpen, setIsSettingsOpen] = useState(false);
    const [isEngineHealthOpen, setIsEngineHealthOpen] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [validationErrors, setValidationErrors] = useState<
        ValidationError[] | null
    >(null);

    const [scopeViews, setScopeViews] = useState<ScopeView[]>([]);
    const [runningBufferId, setRunningBufferId] = useState<string | null>(null);
    const [sliderDefs, setSliderDefs] = useState<SliderDefinition[]>([]);
    const [transportState, setTransportState] =
        useState<TransportSnapshot | null>(null);

    const [updateState, setUpdateState] = useState<UpdateNotificationState>({
        status: 'idle',
    });
    // Store the version currently being offered so we can reference it later
    const pendingUpdateVersion = useRef('');

    const editorRef = useRef<editor.IStandaloneCodeEditor>(null);
    const scopeCanvasMapRef = useRef(new Map<string, HTMLCanvasElement>());
    const lastPatchResultRef = useRef<any>(null);

    /** Long-lived invisible tracked decorations spanning each scope() call.
     *  Monaco automatically adjusts these ranges as the document is edited,
     *  so we can always read the current position of a scope call from them. */
    const scopeDecorationsRef =
        useRef<editor.IEditorDecorationsCollection | null>(null);

    /** Pending UI state waiting for the audio thread to apply a queued update */
    const pendingUIStateRef = useRef<{
        updateId: number;
        scopeViews: ScopeView[];
        sliderDefs: SliderDefinition[];
        interpolationResolutions?: Map<string, any[]>;
        /** Tracked decorations created at submit time, swapped into
         *  scopeDecorationsRef when the pending state is committed. */
        scopeDecorations: editor.IEditorDecorationsCollection | null;
    } | null>(null);

    const handleSliderChange = useCallback(
        (label: string, newValue: number) => {
            // Find the slider definition
            const slider = sliderDefs.find((s) => s.label === label);
            if (!slider) {
                return;
            }

            // Update audio engine via lightweight param update
            void electronAPI.synthesizer.setModuleParam(
                slider.moduleId,
                '$signal',
                {
                    source: newValue,
                },
            );

            // Update the source code in the editor
            const editorInstance = editorRef.current;
            if (editorInstance) {
                const model = editorInstance.getModel();
                if (model) {
                    const source = model.getValue();
                    const span = findSliderValueSpan(source, label);
                    if (span) {
                        const startPos = model.getPositionAt(span.start);
                        const endPos = model.getPositionAt(span.end);
                        const range = new (window as any).monaco.Range(
                            startPos.lineNumber,
                            startPos.column,
                            endPos.lineNumber,
                            endPos.column,
                        );
                        const formattedValue = Number(
                            newValue.toPrecision(6),
                        ).toString();
                        // Use pushEditOperations for proper undo stack integration
                        model.pushEditOperations(
                            [],
                            [{ range, text: formattedValue }],
                            () => null,
                        );
                    }
                }
            }

            // Update slider state
            setSliderDefs((prev) =>
                prev.map((s) =>
                    s.label === label ? { ...s, value: newValue } : s,
                ),
            );
        },
        [sliderDefs],
    );

    // Load workspace and file tree on mount
    useEffect(() => {
        electronAPI.filesystem
            .getWorkspace()
            .then((workspace) => {
                if (workspace) {
                    setWorkspaceRoot(workspace.path);
                    void refreshFileTree();
                }
            })
            .catch((err) => {
                console.error('Failed to load workspace:', err);
            });
    }, [refreshFileTree]);

    // Refresh file tree when wavs/ folder changes
    useEffect(() => {
        const unsubscribe = electronAPI.onWavsChange(() => {
            void refreshFileTree();
        });
        return unsubscribe;
    }, [refreshFileTree]);

    const selectWorkspaceFolder = useCallback(async () => {
        // Check for dirty file-backed buffers before switching
        const dirtyFileBuffers = buffers.filter(
            (b) => b.kind === 'file' && b.dirty,
        );

        if (dirtyFileBuffers.length > 0) {
            const fileList = dirtyFileBuffers
                .map((b) => (b.kind === 'file' ? b.filePath : ''))
                .filter(Boolean)
                .join(', ');

            const response =
                await electronAPI.showUnsavedChangesDialog(fileList);

            if (response === 2) {
                // Cancel / Escape: abort the open workspace operation
                return;
            } else if (response === 0) {
                // Save all dirty file buffers
                for (const buffer of dirtyFileBuffers) {
                    if (buffer.kind === 'file') {
                        await electronAPI.filesystem.writeFile(
                            buffer.filePath,
                            buffer.content,
                        );
                    }
                }
                // Mark them clean
                setBuffers((prev) =>
                    prev.map((b) =>
                        b.kind === 'file' && b.dirty
                            ? { ...b, dirty: false }
                            : b,
                    ),
                );
            } else {
                // Don't Save: discard dirty file buffers
                setBuffers((prev) =>
                    prev.filter((b) => !(b.kind === 'file' && b.dirty)),
                );
            }
        }

        const workspace = await electronAPI.filesystem.selectWorkspace();
        if (workspace) {
            setWorkspaceRoot(workspace.path);
            await refreshFileTree();
        }
    }, [buffers, refreshFileTree, setBuffers]);

    const handleOpenFile = useCallback(
        async (relPath: string, options?: { preview?: boolean }) => {
            try {
                await openFile(relPath, options);
            } catch {
                setError(`Failed to open file: ${relPath}`);
            }
        },
        [openFile],
    );

    const handleDeleteFile = useCallback(
        async (targetIdOrPath?: string) => {
            try {
                await deleteFile(targetIdOrPath);
            } catch (err) {
                setError(getErrorMessage(err, 'Failed to delete file'));
            }
        },
        [deleteFile],
    );

    const formatLabel = useCallback(
        (buffer: EditorBuffer) => {
            const path = formatFileLabel(buffer);
            const parts = path.split(/[/\\]/);
            return parts[parts.length - 1];
        },
        [formatFileLabel],
    );

    const handleRenameCommitSafe = useCallback(
        async (oldPath: string, newName: string) => {
            try {
                await handleRenameCommit(oldPath, newName);
            } catch (err) {
                setError(getErrorMessage(err, 'Failed to rename file'));
            }
        },
        [handleRenameCommit],
    );

    // Handle context menu commands
    useEffect(
        () =>
            electronAPI.onContextMenuCommand((action) => {
                switch (action.command) {
                    case 'save':
                        saveFile(action.bufferId).catch((err) => {
                            setError(
                                getErrorMessage(err, 'Failed to save file'),
                            );
                        });
                        break;
                    case 'rename':
                        renameFile(action.path || action.bufferId).catch(
                            (err) => {
                                setError(
                                    getErrorMessage(
                                        err,
                                        'Failed to rename file',
                                    ),
                                );
                            },
                        );
                        break;
                    case 'delete':
                        deleteFile(action.path || action.bufferId).catch(
                            (err) => {
                                setError(
                                    getErrorMessage(
                                        err,
                                        'Failed to delete file',
                                    ),
                                );
                            },
                        );
                        break;
                }
            }),
        [saveFile, renameFile, deleteFile],
    );

    // Subscribe to update events from main process
    useEffect(() => {
        const unsubAvailable = electronAPI.update.onAvailable(
            (info: UpdateAvailableInfo) => {
                pendingUpdateVersion.current = info.version;
                setUpdateState({
                    releaseUrl: info.releaseUrl,
                    status: 'available',
                    supportsInAppUpdate: info.supportsInAppUpdate,
                    version: info.version,
                });
            },
        );
        const unsubDownloading = electronAPI.update.onDownloading(() => {
            setUpdateState({
                status: 'downloading',
                version: pendingUpdateVersion.current,
            });
        });
        const unsubDownloaded = electronAPI.update.onDownloaded(() => {
            setUpdateState({ status: 'ready' });
        });
        const unsubError = electronAPI.update.onError((message: string) => {
            setUpdateState({ message, status: 'error' });
        });

        return () => {
            unsubAvailable();
            unsubDownloading();
            unsubDownloaded();
            unsubError();
        };
    }, []);

    const handleUpdateDownload = useCallback(() => {
        void electronAPI.update.download();
    }, []);

    const handleUpdateInstall = useCallback(() => {
        void electronAPI.update.install();
    }, []);

    const handleUpdateSkip = useCallback(() => {
        if (pendingUpdateVersion.current) {
            void electronAPI.config.write({
                skippedUpdateVersion: pendingUpdateVersion.current,
            });
        }
        setUpdateState({ status: 'idle' });
    }, []);

    const handleUpdateDismiss = useCallback(() => {
        setUpdateState({ status: 'idle' });
    }, []);

    const registerScopeCanvas = useCallback(
        (key: string, canvas: HTMLCanvasElement) => {
            scopeCanvasMapRef.current.set(key, canvas);
        },
        [],
    );

    const unregisterScopeCanvas = useCallback((key: string) => {
        scopeCanvasMapRef.current.delete(key);
    }, []);

    const patchCodeRef = useRef(patchCode);
    useEffect(() => {
        patchCodeRef.current = patchCode;
    }, [patchCode]);

    const isClockRunningRef = useRef(isClockRunning);
    useEffect(() => {
        isClockRunningRef.current = isClockRunning;
    }, [isClockRunning]);

    useEffect(() => {
        if (!isClockRunningRef.current) {
            return;
        }

        let cancelled = false;
        const tick = () => {
            if (cancelled) return;
            Promise.all([
                electronAPI.synthesizer.getScopes(),
                electronAPI.synthesizer.getTransportState(),
            ])
                .then(([scopeData, transport]) => {
                    if (cancelled) return;
                    // Build a map of buffer key → (Float32Array, ScopeStats)
                    const bufferMap = new Map<
                        string,
                        {
                            data: Float32Array;
                            stats: {
                                min: number;
                                max: number;
                                peakToPeak: number;
                                readOffset: number;
                            };
                        }
                    >();
                    for (const [bufferKey, data, stats] of scopeData) {
                        const key = scopeBufferKeyToString(bufferKey);
                        bufferMap.set(key, { data, stats });
                    }

                    const scopeColors = readScopeColors();

                    // For each scope canvas, collect its channels' data and draw
                    for (const [
                        ,
                        canvas,
                    ] of scopeCanvasMapRef.current.entries()) {
                        const rangeMin = parseFloat(
                            canvas.dataset.scopeRangeMin || '-5',
                        );
                        const rangeMax = parseFloat(
                            canvas.dataset.scopeRangeMax || '5',
                        );
                        const channelKeysStr = canvas.dataset.scopeChannelKeys;
                        if (!channelKeysStr) {
                            continue;
                        }

                        const channelKeys = JSON.parse(
                            channelKeysStr,
                        ) as string[];
                        const channels: Float32Array[] = [];
                        const readOffsets: number[] = [];
                        let globalMin = Infinity;
                        let globalMax = -Infinity;

                        for (const chKey of channelKeys) {
                            const entry = bufferMap.get(chKey);
                            if (entry) {
                                channels.push(entry.data);
                                readOffsets.push(entry.stats.readOffset);
                                if (entry.stats.min < globalMin) {
                                    globalMin = entry.stats.min;
                                }
                                if (entry.stats.max > globalMax) {
                                    globalMax = entry.stats.max;
                                }
                            }
                        }

                        if (channels.length > 0) {
                            drawOscilloscope(channels, canvas, {
                                colors: scopeColors,
                                range: [rangeMin, rangeMax],
                                stats: {
                                    max: globalMax,
                                    min: globalMin,
                                    peakToPeak: globalMax - globalMin,
                                    readOffset: readOffsets,
                                },
                            });
                        }
                    }

                    setTransportState(transport);

                    // Check if a pending UI state should be committed
                    const pending = pendingUIStateRef.current;
                    if (
                        pending &&
                        transport.lastAppliedUpdateId >= pending.updateId
                    ) {
                        pendingUIStateRef.current = null;
                        // Swap decoration collections: dispose old, activate pending
                        scopeDecorationsRef.current?.clear();
                        scopeDecorationsRef.current = pending.scopeDecorations;
                        setScopeViews(pending.scopeViews);
                        setSliderDefs(pending.sliderDefs);
                        if (pending.interpolationResolutions) {
                            setActiveInterpolationResolutions(
                                pending.interpolationResolutions,
                            );
                        }
                    }

                    if (isClockRunningRef.current && !cancelled) {
                        requestAnimationFrame(tick);
                    }
                })
                .catch((err) => {
                    console.error('Failed to get scopes:', err);
                    if (isClockRunningRef.current && !cancelled) {
                        requestAnimationFrame(tick);
                    }
                });
        };
        requestAnimationFrame(tick);

        return () => {
            cancelled = true;
        };
    }, [isClockRunning]);

    // Keep Link phase indicator live while Link is enabled but Operator is stopped.
    // The main tick loop only runs when isClockRunning; this fills the gap so
    // the phase indicator stays animated even before the user presses play.
    useEffect(() => {
        const linkEnabled = transportState?.linkEnabled ?? false;
        if (!linkEnabled || isClockRunning) return;
        let cancelled = false;
        let rafId = 0;
        const tick = () => {
            if (cancelled) return;
            void electronAPI.synthesizer.getTransportState().then((t) => {
                if (cancelled) return;
                setTransportState(t);
                rafId = requestAnimationFrame(tick);
            });
        };
        rafId = requestAnimationFrame(tick);
        return () => {
            cancelled = true;
            cancelAnimationFrame(rafId);
        };
    }, [transportState?.linkEnabled, isClockRunning]);

    const handleSaveFile = useCallback(
        async (id?: string) => {
            try {
                await saveFile(id);
            } catch (err) {
                setError(getErrorMessage(err, 'Failed to save file'));
            }
        },
        [saveFile],
    );

    const handleSaveFileRef = useRef(() => {});
    useEffect(() => {
        handleSaveFileRef.current = handleSaveFile;
    }, [handleSaveFile]);
    const handleSaveFileStable = useCallback(
        () => handleSaveFileRef.current(),
        [],
    );

    const handleOpenWorkspaceRef = useRef(() => {});
    useEffect(() => {
        handleOpenWorkspaceRef.current = selectWorkspaceFolder;
    }, [selectWorkspaceFolder]);

    const handleSubmitRef = useRef((_trigger?: QueuedTrigger) => {});
    useEffect(() => {
        handleSubmitRef.current = async (trigger?: QueuedTrigger) => {
            if (!activeBufferId) {
                return;
            }
            try {
                const patchCodeValue = patchCodeRef.current;

                // Execute DSL in main process (has direct N-API access)
                const result = await electronAPI.executeDSL(
                    patchCodeValue,
                    activeBufferId,
                    trigger,
                );
                lastPatchResultRef.current = result;

                if (!result.success) {
                    // Still set interpolation resolutions even on validation errors
                    // (the analysis succeeded, only the patch application failed)
                    if (result.interpolationResolutions) {
                        const map = new Map(
                            Object.entries(result.interpolationResolutions),
                        );
                        setActiveInterpolationResolutions(map);
                    }
                    if (result.errorMessage) {
                        setError(result.errorMessage);
                        setValidationErrors(null);
                    } else if (result.errors && result.errors.length > 0) {
                        // Extract and transform validation errors to show source lines
                        const rawErrors = result.errors.flatMap(
                            (e) => e.errors || [],
                        );
                        const transformedErrors =
                            transformErrorsWithSourceLocations(
                                rawErrors,
                                result.sourceLocationMap,
                            );
                        setValidationErrors(transformedErrors);
                        setError(
                            result.errors.map((e) => e.message).join('\n') ||
                                'Failed to apply patch.',
                        );
                    }
                    return;
                }

                setIsClockRunning(true);
                setRunningBufferId(activeBufferId);
                setError(null);
                setValidationErrors(null);

                // Set interpolation resolutions in renderer for template literal highlighting
                const interpolationMap = result.interpolationResolutions
                    ? new Map(Object.entries(result.interpolationResolutions))
                    : undefined;

                const scopes = result.appliedPatch?.scopes || [];
                const { callSiteSpans } = result;

                const editorInstance = editorRef.current;
                const model = editorInstance?.getModel();
                const views: ScopeView[] = [];
                const decorationDescs: editor.IModelDeltaDecoration[] = [];

                for (let i = 0; i < scopes.length; i++) {
                    const scope = scopes[i];

                    // Derive buffer keys for each channel in this scope
                    const channelKeys = scope.channels.map((ch: any) =>
                        scopeBufferKeyFromChannel(
                            ch,
                            scope.msPerFrame,
                            scope.triggerThreshold,
                        ),
                    );

                    // Use first channel's key as the scope's identity
                    const scopeKey =
                        channelKeys.length > 0
                            ? `scope:${i}:${channelKeys.join('+')}`
                            : `scope:${i}:empty`;

                    const loc = (scope as any).sourceLocation as
                        | { line: number; column: number }
                        | undefined;

                    views.push({
                        channelKeys,
                        file: activeBufferId,
                        key: scopeKey,
                        range: scope.range ?? [-5, 5],
                    });

                    if (model && loc) {
                        const spanKey = `${loc.line}:${loc.column}`;
                        const callSpan = callSiteSpans?.[spanKey];
                        const endLine = callSpan?.endLine ?? loc.line;

                        const endLineContent =
                            model.getLineContent(endLine) ?? '';
                        decorationDescs.push({
                            options: {
                                stickiness:
                                    editor.TrackedRangeStickiness
                                        .NeverGrowsWhenTypingAtEdges,
                            },
                            range: {
                                endColumn: endLineContent.length + 1,
                                endLineNumber: endLine,
                                startColumn: loc.column,
                                startLineNumber: loc.line,
                            },
                        });
                    }
                }

                let newScopeDecorations: editor.IEditorDecorationsCollection | null =
                    null;
                if (editorInstance && decorationDescs.length > 0) {
                    newScopeDecorations =
                        editorInstance.createDecorationsCollection(
                            decorationDescs,
                        );
                }

                const newSliderDefs = result.sliders ?? [];

                // For queued (non-immediate) triggers, defer UI state until the
                // Audio thread actually applies the patch update.
                const isDeferred =
                    trigger === 'NextBar' || trigger === 'NextBeat';

                if (isDeferred && result.updateId != null) {
                    // Stash both new decorations and UI state; keep old
                    // Decorations alive so the current view zones still work.
                    // Any previously pending (but never committed) decorations
                    // Are cleaned up before storing the new pending state.
                    pendingUIStateRef.current?.scopeDecorations?.clear();
                    pendingUIStateRef.current = {
                        interpolationResolutions: interpolationMap,
                        scopeDecorations: newScopeDecorations,
                        scopeViews: views,
                        sliderDefs: newSliderDefs,
                        updateId: result.updateId,
                    };
                } else {
                    // Immediate trigger (or button click): swap decorations
                    // And apply UI state right away.
                    pendingUIStateRef.current?.scopeDecorations?.clear();
                    pendingUIStateRef.current = null;
                    scopeDecorationsRef.current?.clear();
                    scopeDecorationsRef.current = newScopeDecorations;
                    setScopeViews(views);
                    setSliderDefs(newSliderDefs);
                    if (interpolationMap) {
                        setActiveInterpolationResolutions(interpolationMap);
                    }
                }
            } catch (err) {
                setError(getErrorMessage(err, 'Unknown error'));
                setValidationErrors(null);
            }
        };
    }, [activeBufferId]);

    // Expose test API for E2E tests
    useEffect(() => {
        window.__TEST_API__ = {
            executePatch: async () => {
                handleSubmitRef.current();
            },
            getAudioHealth: () => electronAPI.synthesizer.getHealth(),
            getEditorValue: () => editorRef.current?.getValue() ?? '',
            getLastPatchResult: () => lastPatchResultRef.current,
            getScopeData: () => electronAPI.synthesizer.getScopes(),
            isClockRunning: () => isClockRunningRef.current,
            openEngineHealth: () => setIsEngineHealthOpen(true),
            setEditorValue: (code: string) => editorRef.current?.setValue(code),
        };
        return () => {
            delete window.__TEST_API__;
        };
    }, []);

    const handleStopRef = useRef(() => {});
    useEffect(() => {
        handleStopRef.current = async () => {
            await electronAPI.synthesizer.stop();
            setIsClockRunning(false);
            setRunningBufferId(null);
        };
    }, []);
    const handleStop = useCallback(() => handleStopRef.current(), []);

    const dismissError = useCallback(() => {
        setError(null);
        setValidationErrors(null);
    }, []);

    const handleCloseBuffer = useCallback(
        async (id: string) => {
            await closeBuffer(id);
        },
        [closeBuffer],
    );

    useEffect(() => {
        const cleanupNewFile = electronAPI.onMenuNewFile(() => {
            createUntitledFile();
        });
        const cleanupSave = electronAPI.onMenuSave(() => {
            handleSaveFileRef.current();
        });
        const cleanupStop = electronAPI.onMenuStop(() => {
            handleStopRef.current();
        });
        const cleanupUpdate = electronAPI.onMenuUpdatePatch(() => {
            handleSubmitRef.current('NextBar');
        });
        const cleanupUpdateNextBeat = electronAPI.onMenuUpdatePatchNextBeat(
            () => {
                handleSubmitRef.current('NextBeat');
            },
        );
        const cleanupOpenWorkspace = electronAPI.onMenuOpenWorkspace(() => {
            handleOpenWorkspaceRef.current();
        });
        const cleanupCloseBuffer = electronAPI.onMenuCloseBuffer(() => {
            if (activeBufferId) {
                void handleCloseBuffer(activeBufferId);
            }
        });
        const cleanupToggleRecording = electronAPI.onMenuToggleRecording(() => {
            if (isRecording) {
                void electronAPI.synthesizer.stopRecording();
                setIsRecording(false);
            } else {
                void electronAPI.synthesizer.startRecording();
                setIsRecording(true);
            }
        });

        // Handle opening settings from menu (Cmd+,)
        const cleanupOpenSettings = electronAPI.onMenuOpenSettings(() => {
            setIsSettingsOpen(true);
        });
        const cleanupOpenEngineHealth = electronAPI.onMenuOpenEngineHealth(
            () => {
                setIsEngineHealthOpen(true);
            },
        );

        return () => {
            cleanupNewFile();
            cleanupSave();
            cleanupStop();
            cleanupUpdate();
            cleanupUpdateNextBeat();
            cleanupOpenWorkspace();
            cleanupCloseBuffer();
            cleanupToggleRecording();
            cleanupOpenSettings();
            cleanupOpenEngineHealth();
        };
    }, [
        activeBufferId,
        handleCloseBuffer,
        isRecording,
        buffers,
        createUntitledFile,
    ]);

    // Ctrl+Enter (and Ctrl+Shift+Enter) are reserved for patch updates.
    // Browsers activate a focused <button> on Enter regardless of modifier
    // state, which would spuriously toggle e.g. the Link button after it had
    // been clicked. Suppress the default activation when a button is focused.
    useEffect(() => {
        const onKeyDownCapture = (e: KeyboardEvent) => {
            if (
                e.ctrlKey &&
                e.key === 'Enter' &&
                e.target instanceof HTMLButtonElement
            ) {
                e.preventDefault();
                e.stopPropagation();
            }
        };
        window.addEventListener('keydown', onKeyDownCapture, { capture: true });
        return () =>
            window.removeEventListener('keydown', onKeyDownCapture, {
                capture: true,
            });
    }, []);

    return (
        <div className="app">
            <header className="app-header">
                <TransportDisplay
                    transport={transportState}
                    onToggleLink={(enabled) => {
                        void electronAPI.synthesizer.enableLink(enabled);
                        // Optimistically update UI — polling only runs while playing
                        setTransportState((prev) =>
                            prev
                                ? {
                                      ...prev,
                                      linkEnabled: enabled,
                                      linkPeers: enabled ? prev.linkPeers : 0,
                                  }
                                : prev,
                        );
                    }}
                />
                <AudioControls
                    isRunning={isClockRunning}
                    isRecording={isRecording}
                    onStop={handleStop}
                    onStartRecording={async () => {
                        await electronAPI.synthesizer.startRecording();
                        setIsRecording(true);
                    }}
                    onStopRecording={async () => {
                        await electronAPI.synthesizer.stopRecording();
                        setIsRecording(false);
                    }}
                    onUpdatePatch={() => handleSubmitRef.current()}
                />
            </header>

            <ErrorDisplay
                error={error}
                errors={validationErrors}
                onDismiss={dismissError}
            />

            <Settings
                isOpen={isSettingsOpen}
                onClose={() => setIsSettingsOpen(false)}
            />

            <EngineHealth
                isOpen={isEngineHealthOpen}
                onClose={() => setIsEngineHealthOpen(false)}
            />

            <main className="app-main">
                {!workspaceRoot ? (
                    <div className="empty-state">
                        <button
                            className="open-folder-button"
                            onClick={selectWorkspaceFolder}
                        >
                            Open Folder
                        </button>
                    </div>
                ) : (
                    <>
                        <div className="editor-panel">
                            <PatchEditor
                                value={patchCode}
                                runningBufferId={runningBufferId}
                                currentFile={activeBufferId}
                                onChange={handlePatchChange}
                                editorRef={editorRef}
                                scopeViews={scopeViews}
                                // oxlint-disable-next-line react-hooks-js/refs -- intentional: live Monaco decoration collection mutated outside React
                                scopeDecorations={scopeDecorationsRef.current}
                                onRegisterScopeCanvas={registerScopeCanvas}
                                onUnregisterScopeCanvas={unregisterScopeCanvas}
                            />
                        </div>

                        <Sidebar
                            explorerContent={
                                <FileExplorer
                                    workspaceRoot={workspaceRoot}
                                    fileTree={fileTree}
                                    buffers={buffers}
                                    activeBufferId={activeBufferId}
                                    runningBufferId={runningBufferId}
                                    renamingPath={renamingPath}
                                    formatLabel={formatLabel}
                                    onSelectBuffer={setActiveBufferId}
                                    onOpenFile={handleOpenFile}
                                    onCreateFile={createUntitledFile}
                                    onSaveFile={handleSaveFileStable}
                                    onRenameFile={renameFile}
                                    onDeleteFile={handleDeleteFile}
                                    onCloseBuffer={handleCloseBuffer}
                                    onSelectWorkspace={selectWorkspaceFolder}
                                    onRefreshTree={refreshFileTree}
                                    onRenameCommit={handleRenameCommitSafe}
                                    onRenameCancel={() => setRenamingPath(null)}
                                    onKeepBuffer={keepBuffer}
                                />
                            }
                            controlContent={
                                <ControlPanel
                                    sliders={sliderDefs}
                                    onSliderChange={handleSliderChange}
                                />
                            }
                        />
                    </>
                )}
            </main>
            <UpdateNotification
                state={updateState}
                onDownload={handleUpdateDownload}
                onInstall={handleUpdateInstall}
                onSkip={handleUpdateSkip}
                onDismiss={handleUpdateDismiss}
            />
        </div>
    );
}

export default App;
