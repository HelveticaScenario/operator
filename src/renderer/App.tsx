import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { MonacoPatchEditor as PatchEditor } from './components/MonacoPatchEditor';
import { AudioControls } from './components/AudioControls';
import { TransportDisplay } from './components/TransportDisplay';
import { ErrorDisplay } from './components/ErrorDisplay';
import { Settings } from './components/Settings';
import { AudioPanicDialog } from './components/AudioPanicDialog';
import { EngineHealth } from './components/EngineHealth';
import { ModuleProfile } from './components/ModuleProfile';
import { MigrationDiffModal } from './components/MigrationDiffModal';
import type { MigrationModalSummary } from './components/MigrationDiffModal';
import { migrateChebyBlockDC } from './dsl/migrateChebyBlockDC';
import { migrateCycleCalls } from './dsl/migrateCycleCalls';
import { migrateWavetableArgs } from './dsl/migrateWavetableArgs';
import type { UpdateNotificationState } from './components/UpdateNotification';
import { UpdateNotification } from './components/UpdateNotification';
import { CommandPalette } from './components/CommandPalette';
import { ScopeXYBackground } from './app/scopexy/ScopeXYBackground';
import './App.css';
import { editor } from 'monaco-editor';
import { getErrorMessage } from './utils/errorUtils';
import { FileExplorer } from './components/FileExplorer';
import { Sidebar } from './components/Sidebar';
import { ControlPanel } from './components/ControlPanel';
import electronAPI from './electronAPI';
import type { ValidationError } from '@modular/core';
import type { QueuedTrigger } from '@modular/core';
import type { FileTreeEntry, UpdateAvailableInfo } from '../shared/ipcTypes';
import type { SliderDefinition } from '../shared/dsl/sliderTypes';
import type { VuMeterDef, VuMeterGhost } from '../shared/dsl/vuMeterTypes';
import {
    UNITY_OUT_GAIN,
    dbToOutGain,
    outGainToDb,
} from '../shared/dsl/vuMeterTypes';
import type { EditorBuffer } from './types/editor';
import { applySliderChange } from './app/sliderChange';
import { resolveScopeCallRange } from './app/scopeCallRange';
import { transformErrorsWithSourceLocations } from './app/validationErrorLocations';
import {
    computeOutNumericOptionEdit,
    computeOutOptionEdit,
    computeSetOutputGainEdit,
} from './dsl/outSourceEdit';

/** The GraphBuilder's output-gain default, restored by a master-fader reset. */
const DEFAULT_OUTPUT_GAIN = 2.5;
import {
    VU_PANEL_MAX_HEIGHT,
    VU_PANEL_MIN_HEIGHT,
    VuMeterPanel,
} from './components/VuMeterPanel';
import type { VuBallistics } from './app/vuMeter';
import {
    drawVuMeter,
    formatDb,
    newBallistics,
    readVuMeterColors,
    updateBallistics,
    voltsToDb,
} from './app/vuMeter';
import type { ScopeView } from './types/editor';
import {
    FIRST_LINE_COLUMN_OFFSET,
    setActiveInterpolationResolutions,
} from '../shared/dsl/spanTypes';
import {
    drawOscilloscope,
    readScopeColors,
    scopeBufferKeyFromChannel,
    scopeBufferKeyToString,
} from './app/oscilloscope';
import { useEditorBuffers } from './app/hooks/useEditorBuffers';
import {
    executeCommand,
    registerCommand,
    unregisterCommand,
} from './keybindings/commands';
import { loadAndInstallKeymap, setWhenEvaluator } from './keybindings/keymap';
import { evaluateWhen } from './keybindings/contextKey';
import { getBufferId } from './app/buffers';
import {
    setTransport,
    updateTransport,
    useTransportLinkEnabled,
} from './app/transportStore';
import { useTheme } from './themes/ThemeContext';

function App() {
    const {
        xyScopeIntensity,
        xyScopePersistence,
        xyScopeUpsample,
        xyScopeLineWidth,
    } = useTheme();

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

    // Absolute path to the user keybindings.json, resolved once. Used to
    // detect when that file is saved so the keymap can be reloaded live.
    const keybindingsPathRef = useRef<string | null>(null);
    const handleFileSaved = useCallback((filePath: string) => {
        if (filePath === keybindingsPathRef.current) {
            window.dispatchEvent(new Event('operator:keybindings-changed'));
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
        openAbsoluteFile,
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
    } = useEditorBuffers({
        refreshFileTree,
        workspaceRoot,
        onFileSaved: handleFileSaved,
    });

    // Audio state
    const [isClockRunning, setIsClockRunning] = useState(true);
    const [isRecording, setIsRecording] = useState(false);
    const [isSettingsOpen, setIsSettingsOpen] = useState(false);
    const [isEngineHealthOpen, setIsEngineHealthOpen] = useState(false);
    const [isPaletteOpen, setIsPaletteOpen] = useState(false);
    const [isModuleProfileOpen, setIsModuleProfileOpen] = useState(false);
    const [migrationState, setMigrationState] = useState<{
        bufferId: string;
        original: string;
        migrated: string;
        summary: MigrationModalSummary;
        title?: string;
        skippedLabel?: string;
    } | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [validationErrors, setValidationErrors] = useState<
        ValidationError[] | null
    >(null);

    const [scopeViews, setScopeViews] = useState<ScopeView[]>([]);
    // Path-identity (getBufferId) of the buffer the running patch came from,
    // compared against activeBufferId by every consumer. Path identities
    // mutate on save/rename, so the stable EditorBuffer.id of the running
    // buffer is tracked alongside and the effect below re-derives this value
    // whenever the buffer's path identity changes.
    const [runningBufferId, setRunningBufferId] = useState<string | null>(null);
    const runningSourceIdRef = useRef<string | null>(null);
    const [sliderDefs, setSliderDefs] = useState<SliderDefinition[]>([]);
    const [vuOutputs, setVuOutputs] = useState<VuMeterDef[]>([]);
    const [isVuPanelVisible, setIsVuPanelVisible] = useState(false);
    const [vuPanelHeight, setVuPanelHeight] = useState(150);
    /** Code-side control values diverging from the running audio after
     *  Ctrl/Cmd (code-only) panel edits, keyed by meter key. Cleared when a
     *  patch update applies. */
    const [vuGhosts, setVuGhosts] = useState(new Map<string, VuMeterGhost>());
    // Per-frame transport lives in an external store (see transportStore) so
    // updating it ~60×/s does not re-render the whole App tree — only the
    // transport display, which subscribes directly. App only needs to know
    // whether Link is enabled (to drive the Link-only poll loop below); that
    // selector re-renders App only when the flag flips.
    const linkEnabled = useTransportLinkEnabled();

    const [updateState, setUpdateState] = useState<UpdateNotificationState>({
        status: 'idle',
    });
    // Store the version currently being offered so we can reference it later
    const pendingUpdateVersion = useRef('');

    const editorRef = useRef<editor.IStandaloneCodeEditor>(null);
    // Mirror of editorRef as state so children that need to react when the
    // editor mounts/unmounts (e.g., the command palette enumerating Monaco
    // editor actions) can subscribe instead of reading the ref during render.
    const [paletteEditor, setPaletteEditor] =
        useState<editor.IStandaloneCodeEditor | null>(null);
    const scopeCanvasMapRef = useRef(new Map<string, HTMLCanvasElement>());
    const vuCanvasMapRef = useRef(new Map<string, HTMLCanvasElement>());
    /** Peak-readout pills, updated imperatively from the RAF loop. */
    const vuReadoutMapRef = useRef(new Map<string, HTMLElement>());
    /** Locked pan knobs' pointer lines, rotated live from the RAF loop. */
    const vuPanPointerMapRef = useRef(new Map<string, SVGLineElement>());
    /** Mirror of vuGhosts for the RAF loop and drag handlers; updated
     *  synchronously by setVuGhostProp/clearVuGhosts so a redraw issued in
     *  the same tick sees the new value. */
    const vuGhostsRef = useRef(vuGhosts);
    /** Throttle state for live source writes during fader/knob drags,
     *  keyed `<meterKey>:<control>`. Each Monaco edit re-tokenizes and
     *  re-colors, so drags write the source at a bounded rate — immediately,
     *  then at most once per interval while moving; release flushes the
     *  final value. */
    const vuEditThrottleRef = useRef(
        new Map<string, { timer: number | null; lastWrite: number }>(),
    );
    /** Per-meter, per-channel peak-marker state keyed by meter key. */
    const vuBallisticsRef = useRef(new Map<string, VuBallistics[]>());
    /** Last drawn channel levels per meter, for redraws outside the RAF
     *  loop (canvas resizes and fader drags while the clock is stopped). */
    const vuLastChannelsRef = useRef(
        new Map<
            string,
            { rmsDb: number; fastDb: number; peakDb: number }[]
        >(),
    );
    const lastPatchResultRef = useRef<any>(null);

    /** Long-lived invisible tracked decorations spanning each scope() call.
     *  Monaco automatically adjusts these ranges as the document is edited,
     *  so we can always read the current position of a scope call from them. */
    const scopeDecorationsRef =
        useRef<editor.IEditorDecorationsCollection | null>(null);

    /** Tracked decorations spanning each out()/outMono() call, index-aligned
     *  with vuOutputs; the VU meter M/S buttons edit the source through them. */
    const vuDecorationsRef =
        useRef<editor.IEditorDecorationsCollection | null>(null);

    /** Pending UI state waiting for the audio thread to apply a queued update */
    const pendingUIStateRef = useRef<{
        updateId: number;
        scopeViews: ScopeView[];
        sliderDefs: SliderDefinition[];
        vuOutputs: VuMeterDef[];
        interpolationResolutions?: Map<string, any[]>;
        /** Tracked decorations created at submit time, swapped into
         *  scopeDecorationsRef when the pending state is committed. */
        scopeDecorations: editor.IEditorDecorationsCollection | null;
        /** Same contract as scopeDecorations, for vuDecorationsRef. */
        vuDecorations: editor.IEditorDecorationsCollection | null;
    } | null>(null);

    const handleSliderChange = useCallback(
        (label: string, newValue: number) => {
            const slider = sliderDefs.find((s) => s.label === label);
            if (!slider) {
                return;
            }

            // The editor shows the active buffer, which is not necessarily
            // the buffer the running patch (and its sliders) came from.
            applySliderChange(
                slider,
                newValue,
                editorRef.current?.getModel() ?? null,
                activeBufferId !== undefined &&
                    activeBufferId === runningBufferId,
                (moduleId, moduleType, params) => {
                    void electronAPI.synthesizer.setModuleParam(
                        moduleId,
                        moduleType,
                        params,
                    );
                },
            );

            // Update slider state
            setSliderDefs((prev) =>
                prev.map((s) =>
                    s.label === label ? { ...s, value: newValue } : s,
                ),
            );
        },
        [sliderDefs, activeBufferId, runningBufferId],
    );

    // Mirrors for the RAF loop and the M/S click handler, which must read
    // current values without re-subscribing.
    const vuOutputsRef = useRef<VuMeterDef[]>([]);
    useEffect(() => {
        vuOutputsRef.current = vuOutputs;
    }, [vuOutputs]);
    const isVuPanelVisibleRef = useRef(false);
    useEffect(() => {
        isVuPanelVisibleRef.current = isVuPanelVisible;
    }, [isVuPanelVisible]);

    /** Set or clear (`value` undefined) one ghost property for a meter. The
     *  ref updates synchronously so a redraw in the same tick sees it. */
    const setVuGhostProp = useCallback(
        <P extends keyof VuMeterGhost>(
            key: string,
            prop: P,
            value: VuMeterGhost[P] | undefined,
        ) => {
            const prev = vuGhostsRef.current;
            if (value === undefined && prev.get(key)?.[prop] === undefined) {
                return;
            }
            const entry: VuMeterGhost = { ...prev.get(key) };
            if (value === undefined) {
                delete entry[prop];
            } else {
                entry[prop] = value;
            }
            const next = new Map(prev);
            if (Object.keys(entry).length === 0) {
                next.delete(key);
            } else {
                next.set(key, entry);
            }
            vuGhostsRef.current = next;
            setVuGhosts(next);
        },
        [],
    );

    /** Drop every ghost — an applied patch update re-syncs audio to code. */
    const clearVuGhosts = useCallback(() => {
        if (vuGhostsRef.current.size === 0) {
            return;
        }
        vuGhostsRef.current = new Map();
        setVuGhosts(vuGhostsRef.current);
    }, []);

    /** Apply a character-offset edit to the editor, keeping the user's
     *  selections in place (the default would park the cursor at the end of
     *  the edited span) and the edit on the undo stack. */
    const pushVuModelEdit = useCallback(
        (edit: { start: number; end: number; text: string }) => {
            const editorInstance = editorRef.current;
            const model = editorInstance?.getModel();
            if (!model) {
                return;
            }
            const startPos = model.getPositionAt(edit.start);
            const endPos = model.getPositionAt(edit.end);
            const editRange = new (window as any).monaco.Range(
                startPos.lineNumber,
                startPos.column,
                endPos.lineNumber,
                endPos.column,
            );
            const selections = editorInstance?.getSelections() ?? null;
            model.pushEditOperations(
                selections ?? [],
                [{ range: editRange, text: edit.text }],
                () => selections,
            );
        },
        [],
    );

    /**
     * Mirror a VU control change into the DSL source at the out call's
     * tracked decoration. Returns whether an edit was written; skipped
     * silently when the call site can't be edited (dynamic call,
     * non-literal value, stale anchor).
     */
    const applyVuSourceEdit = useCallback(
        (
            idx: number,
            outputs: VuMeterDef[],
            makeEdit: (
                source: string,
                anchorOffset: number,
            ) => { start: number; end: number; text: string } | null,
        ): boolean => {
            const model = editorRef.current?.getModel();
            const range = vuDecorationsRef.current?.getRange(idx);
            if (!model || !range || !outputs[idx].sourceLocation) {
                return false;
            }
            const anchorOffset = model.getOffsetAt({
                column: range.startColumn,
                lineNumber: range.startLineNumber,
            });
            const edit = makeEdit(model.getValue(), anchorOffset);
            if (!edit) {
                return false;
            }
            pushVuModelEdit(edit);
            return true;
        },
        [pushVuModelEdit],
    );

    /**
     * VU meter M/S click. A plain click flips the property, live-updates
     * every mute gate whose effective value changed (solo is global), and
     * mirrors the change into the DSL source at the call's tracked
     * decoration — the same edit-code + edit-live-graph contract as sliders,
     * with no patch re-eval. A code-only click edits the source alone: the
     * audio keeps running unchanged and the button's outer (code) section
     * ghosts until a patch update applies the new value.
     */
    const handleVuToggle = useCallback(
        (key: string, prop: 'mute' | 'solo', codeOnly: boolean) => {
            const outputs = vuOutputsRef.current;
            const idx = outputs.findIndex((o) => o.key === key);
            if (idx === -1) {
                return;
            }

            if (codeOnly) {
                const audioValue = outputs[idx][prop];
                const nextCode = !(
                    vuGhostsRef.current.get(key)?.[prop] ?? audioValue
                );
                const edited = applyVuSourceEdit(
                    idx,
                    outputs,
                    (source, anchorOffset) =>
                        computeOutOptionEdit(
                            source,
                            anchorOffset,
                            prop,
                            nextCode,
                        ),
                );
                if (edited) {
                    setVuGhostProp(
                        key,
                        prop,
                        nextCode === audioValue ? undefined : nextCode,
                    );
                }
                return;
            }

            const next = outputs.map((o, i) =>
                i === idx ? { ...o, [prop]: !o[prop] } : o,
            );

            const anySoloBefore = outputs.some((o) => o.solo);
            const anySoloAfter = next.some((o) => o.solo);
            for (let i = 0; i < next.length; i++) {
                // The master meter has no gate.
                const muteModuleId = next[i].muteModuleId;
                if (muteModuleId == null) {
                    continue;
                }
                const gateBefore = (
                    anySoloBefore ? outputs[i].solo : !outputs[i].mute
                )
                    ? 5
                    : 0;
                const gateAfter = (anySoloAfter ? next[i].solo : !next[i].mute)
                    ? 5
                    : 0;
                if (gateBefore !== gateAfter) {
                    void electronAPI.synthesizer.setModuleParam(
                        muteModuleId,
                        '$signal',
                        { source: gateAfter },
                    );
                }
            }

            applyVuSourceEdit(idx, outputs, (source, anchorOffset) =>
                computeOutOptionEdit(
                    source,
                    anchorOffset,
                    prop,
                    next[idx][prop],
                ),
            );

            // Code and audio agree again for this control.
            setVuGhostProp(key, prop, undefined);

            // Optimistic UI; the source of truth reasserts on the next eval.
            setVuOutputs(next);
        },
        [applyVuSourceEdit, setVuGhostProp],
    );

    /** Interval between live source writes while a control is dragging. */
    const VU_EDIT_INTERVAL_MS = 150;

    /** Throttled live source write for a dragging control: leading edge
     *  fires immediately, further moves fire at most once per interval. */
    const scheduleVuEdit = useCallback(
        (timerKey: string, write: () => void) => {
            const throttles = vuEditThrottleRef.current;
            let state = throttles.get(timerKey);
            if (!state) {
                state = { lastWrite: 0, timer: null };
                throttles.set(timerKey, state);
            }
            if (state.timer !== null) {
                window.clearTimeout(state.timer);
                state.timer = null;
            }
            const elapsed = performance.now() - state.lastWrite;
            if (elapsed >= VU_EDIT_INTERVAL_MS) {
                state.lastWrite = performance.now();
                write();
                return;
            }
            state.timer = window.setTimeout(() => {
                state.timer = null;
                state.lastWrite = performance.now();
                write();
            }, VU_EDIT_INTERVAL_MS - elapsed);
        },
        [],
    );

    /** Cancel a pending throttled write (its final flush is imminent). */
    const cancelVuEdit = useCallback((timerKey: string) => {
        const state = vuEditThrottleRef.current.get(timerKey);
        if (state?.timer != null) {
            window.clearTimeout(state.timer);
            state.timer = null;
        }
    }, []);

    /** Write the current pan into the out call's `pan` option (removed at
     *  center, the default). */
    const writeVuPanEdit = useCallback(
        (key: string, pan: number) => {
            const outputs = vuOutputsRef.current;
            const idx = outputs.findIndex((o) => o.key === key);
            if (idx === -1 || !outputs[idx].panModuleId) {
                return;
            }
            applyVuSourceEdit(idx, outputs, (source, anchorOffset) =>
                computeOutNumericOptionEdit(
                    source,
                    anchorOffset,
                    'pan',
                    pan === 0 ? null : pan,
                ),
            );
        },
        [applyVuSourceEdit],
    );

    /**
     * Pan knob drag: drive the lifted pan $signal live per move; the source
     * write is debounced so the editor re-colors a few times a second at
     * most, not per pointer event. A code-only drag leaves the $signal (and
     * the knob's audio pointer) alone and moves only the ghost pointer,
     * writing the source at the same debounced rate.
     */
    const handleVuPanChange = useCallback(
        (key: string, pan: number, codeOnly: boolean) => {
            const outputs = vuOutputsRef.current;
            const idx = outputs.findIndex((o) => o.key === key);
            if (idx === -1 || !outputs[idx].panModuleId) {
                return;
            }

            if (codeOnly) {
                if (!outputs[idx].sourceLocation) {
                    return;
                }
                setVuGhostProp(
                    key,
                    'pan',
                    pan === outputs[idx].pan ? undefined : pan,
                );
                scheduleVuEdit(`${key}:pan`, () => writeVuPanEdit(key, pan));
                return;
            }

            void electronAPI.synthesizer.setModuleParam(
                outputs[idx].panModuleId,
                '$signal',
                { source: pan },
            );

            const next = outputs.map((o, i) =>
                i === idx ? { ...o, pan } : o,
            );
            vuOutputsRef.current = next;
            setVuOutputs(next);
            setVuGhostProp(key, 'pan', undefined);
            scheduleVuEdit(`${key}:pan`, () => writeVuPanEdit(key, pan));
        },
        [scheduleVuEdit, setVuGhostProp, writeVuPanEdit],
    );

    /** Drag released: flush the final pan into the source immediately. */
    const handleVuPanCommit = useCallback(
        (key: string, pan: number, codeOnly: boolean) => {
            const outputs = vuOutputsRef.current;
            const idx = outputs.findIndex((o) => o.key === key);
            if (
                idx === -1 ||
                (codeOnly && !outputs[idx].sourceLocation)
            ) {
                return;
            }
            cancelVuEdit(`${key}:pan`);
            writeVuPanEdit(key, pan);
        },
        [cancelVuEdit, writeVuPanEdit],
    );

    const handleVuToggleMute = useCallback(
        (key: string, codeOnly: boolean) =>
            handleVuToggle(key, 'mute', codeOnly),
        [handleVuToggle],
    );
    const handleVuToggleSolo = useCallback(
        (key: string, codeOnly: boolean) =>
            handleVuToggle(key, 'solo', codeOnly),
        [handleVuToggle],
    );

    // Restore panel visibility and height from the app config once on mount.
    useEffect(() => {
        electronAPI.config
            .read()
            .then((config) => {
                setIsVuPanelVisible(config.vuPanelVisible ?? false);
                if (config.vuPanelHeight !== undefined) {
                    setVuPanelHeight(
                        Math.min(
                            VU_PANEL_MAX_HEIGHT,
                            Math.max(
                                VU_PANEL_MIN_HEIGHT,
                                config.vuPanelHeight,
                            ),
                        ),
                    );
                }
            })
            .catch((err) => {
                console.error('Failed to read config:', err);
            });
    }, []);

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
        const unsubPreparing = electronAPI.update.onPreparing(() => {
            setUpdateState({
                status: 'preparing',
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
            unsubPreparing();
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

    const registerVuCanvas = useCallback(
        (key: string, canvas: HTMLCanvasElement) => {
            vuCanvasMapRef.current.set(key, canvas);
        },
        [],
    );

    const unregisterVuCanvas = useCallback((key: string) => {
        vuCanvasMapRef.current.delete(key);
        vuBallisticsRef.current.delete(key);
        vuLastChannelsRef.current.delete(key);
    }, []);

    /**
     * Redraw one meter outside the RAF loop, using the last drawn levels (or
     * silence) — for canvas resizes and fader moves while the clock is
     * stopped. `gainDbOverride` paints an in-flight fader value before the
     * optimistic state lands.
     */
    const redrawVuMeter = useCallback(
        (key: string, gainDbOverride?: number | null) => {
            const canvas = vuCanvasMapRef.current.get(key);
            if (!canvas) {
                return;
            }
            const channelCount = Number(canvas.dataset.channels ?? 1);
            const channels =
                vuLastChannelsRef.current.get(key) ??
                Array.from({ length: channelCount }, () => ({
                    fastDb: -Infinity,
                    peakDb: -Infinity,
                    rmsDb: -Infinity,
                }));
            const output = vuOutputsRef.current.find((o) => o.key === key);
            const gainDb =
                gainDbOverride !== undefined
                    ? gainDbOverride
                    : output && output.gain !== null
                      ? outGainToDb(output.gain)
                      : null;
            const ghostGain = vuGhostsRef.current.get(key)?.gain;
            drawVuMeter(
                canvas,
                channels,
                readVuMeterColors(),
                gainDb,
                output?.gainLocked === true,
                ghostGain !== undefined ? outGainToDb(ghostGain) : null,
            );
        },
        [],
    );

    const handleVuCanvasResized = useCallback(
        (key: string) => {
            redrawVuMeter(key);
        },
        [redrawVuMeter],
    );

    /** Write `gain` into the source — the out call's `gain` option, or
     *  $setOutputGain for the master. */
    const writeVuGainEdit = useCallback(
        (key: string, gain: number) => {
            const outputs = vuOutputsRef.current;
            const idx = outputs.findIndex((o) => o.key === key);
            if (idx === -1 || !outputs[idx].gainModuleId) {
                return;
            }
            if (outputs[idx].main) {
                // The master fader's source of truth is $setOutputGain.
                const model = editorRef.current?.getModel();
                if (model) {
                    const edit = computeSetOutputGainEdit(
                        model.getValue(),
                        gain,
                    );
                    if (edit) {
                        pushVuModelEdit(edit);
                    }
                }
            } else {
                applyVuSourceEdit(idx, outputs, (source, anchorOffset) =>
                    computeOutNumericOptionEdit(
                        source,
                        anchorOffset,
                        'gain',
                        gain,
                    ),
                );
            }
        },
        [applyVuSourceEdit, pushVuModelEdit],
    );

    /**
     * Fader drag on a meter: drive the lifted gain $signal live and repaint
     * the triangle immediately (the RAF loop only runs while the clock
     * does); the source write is debounced like the pan knob's. A code-only
     * drag leaves the $signal and the solid triangle alone and moves only
     * the faded ghost triangle, writing the source at the same rate.
     */
    const handleVuGainChange = useCallback(
        (key: string, db: number, codeOnly: boolean) => {
            const outputs = vuOutputsRef.current;
            const idx = outputs.findIndex((o) => o.key === key);
            if (idx === -1 || !outputs[idx].gainModuleId) {
                return;
            }
            // Snap to 0.5 dB steps; the scale floor means silence.
            const snappedDb = Math.round(db * 2) / 2;
            const gain =
                snappedDb <= -60
                    ? 0
                    : Number(dbToOutGain(snappedDb).toPrecision(4));

            if (codeOnly) {
                if (!outputs[idx].main && !outputs[idx].sourceLocation) {
                    return;
                }
                if (gain === vuGhostsRef.current.get(key)?.gain) {
                    return;
                }
                setVuGhostProp(
                    key,
                    'gain',
                    gain === outputs[idx].gain ? undefined : gain,
                );
                redrawVuMeter(key);
                scheduleVuEdit(`${key}:gain`, () =>
                    writeVuGainEdit(key, gain),
                );
                return;
            }

            if (gain === outputs[idx].gain) {
                return;
            }

            void electronAPI.synthesizer.setModuleParam(
                outputs[idx].gainModuleId,
                '$signal',
                { source: gain },
            );

            const next = outputs.map((o, i) =>
                i === idx ? { ...o, gain } : o,
            );
            vuOutputsRef.current = next;
            setVuOutputs(next);
            setVuGhostProp(key, 'gain', undefined);
            redrawVuMeter(key, gain === 0 ? -Infinity : snappedDb);
            scheduleVuEdit(`${key}:gain`, () => writeVuGainEdit(key, gain));
        },
        [redrawVuMeter, scheduleVuEdit, setVuGhostProp, writeVuGainEdit],
    );

    /** Drag released: flush the final gain into the source immediately. */
    const handleVuGainCommit = useCallback(
        (key: string, codeOnly: boolean) => {
            const outputs = vuOutputsRef.current;
            const idx = outputs.findIndex((o) => o.key === key);
            if (idx === -1) {
                return;
            }
            const gain = codeOnly
                ? (vuGhostsRef.current.get(key)?.gain ?? outputs[idx].gain)
                : outputs[idx].gain;
            if (gain === null) {
                return;
            }
            cancelVuEdit(`${key}:gain`);
            writeVuGainEdit(key, gain);
        },
        [cancelVuEdit, writeVuGainEdit],
    );

    /** Ctrl/Cmd right-click on a meter: revert the source to the gain the
     *  audio is running, dropping the ghost; unity reverts by removing the
     *  property. No-op when code and audio agree. */
    const handleVuGainRevert = useCallback(
        (key: string) => {
            const outputs = vuOutputsRef.current;
            const idx = outputs.findIndex((o) => o.key === key);
            if (idx === -1 || !outputs[idx].gainModuleId) {
                return;
            }
            const audioGain = outputs[idx].gain;
            if (
                vuGhostsRef.current.get(key)?.gain === undefined ||
                audioGain === null
            ) {
                return;
            }
            cancelVuEdit(`${key}:gain`);
            if (!outputs[idx].main && audioGain === UNITY_OUT_GAIN) {
                applyVuSourceEdit(idx, outputs, (source, anchorOffset) =>
                    computeOutNumericOptionEdit(
                        source,
                        anchorOffset,
                        'gain',
                        null,
                    ),
                );
            } else {
                writeVuGainEdit(key, audioGain);
            }
            setVuGhostProp(key, 'gain', undefined);
            redrawVuMeter(key);
        },
        [
            applyVuSourceEdit,
            cancelVuEdit,
            redrawVuMeter,
            setVuGhostProp,
            writeVuGainEdit,
        ],
    );

    /** Right-click on a meter: back to the default gain (unity for outs,
     *  the builder's output-gain default for the master), property removed
     *  or rewritten accordingly. */
    const handleVuGainReset = useCallback(
        (key: string) => {
            cancelVuEdit(`${key}:gain`);
            const outputs = vuOutputsRef.current;
            const idx = outputs.findIndex((o) => o.key === key);
            if (idx === -1 || !outputs[idx].gainModuleId) {
                return;
            }
            const isMain = outputs[idx].main === true;
            const resetGain = isMain
                ? DEFAULT_OUTPUT_GAIN
                : UNITY_OUT_GAIN;

            if (isMain) {
                const model = editorRef.current?.getModel();
                if (model) {
                    const edit = computeSetOutputGainEdit(
                        model.getValue(),
                        resetGain,
                    );
                    if (edit) {
                        pushVuModelEdit(edit);
                    }
                }
            } else {
                applyVuSourceEdit(idx, outputs, (source, anchorOffset) =>
                    computeOutNumericOptionEdit(
                        source,
                        anchorOffset,
                        'gain',
                        null,
                    ),
                );
            }

            void electronAPI.synthesizer.setModuleParam(
                outputs[idx].gainModuleId,
                '$signal',
                { source: resetGain },
            );

            const next = outputs.map((o, i) =>
                i === idx ? { ...o, gain: resetGain } : o,
            );
            vuOutputsRef.current = next;
            setVuOutputs(next);
            setVuGhostProp(key, 'gain', undefined);
            redrawVuMeter(key, outGainToDb(resetGain));
        },
        [
            applyVuSourceEdit,
            cancelVuEdit,
            pushVuModelEdit,
            redrawVuMeter,
            setVuGhostProp,
        ],
    );

    const registerVuReadout = useCallback(
        (key: string, el: HTMLElement) => {
            vuReadoutMapRef.current.set(key, el);
        },
        [],
    );

    const unregisterVuReadout = useCallback((key: string) => {
        vuReadoutMapRef.current.delete(key);
    }, []);

    const registerVuPanPointer = useCallback(
        (key: string, el: SVGLineElement) => {
            vuPanPointerMapRef.current.set(key, el);
        },
        [],
    );

    const unregisterVuPanPointer = useCallback((key: string) => {
        vuPanPointerMapRef.current.delete(key);
    }, []);

    /** Restart a meter's peak hold; it re-seeds from the next frame's levels. */
    const handleVuPeakReset = useCallback((key: string) => {
        vuBallisticsRef.current.delete(key);
        const readout = vuReadoutMapRef.current.get(key);
        if (readout) {
            readout.textContent = '-∞';
        }
    }, []);

    const handleVuPanelHeightCommit = useCallback((height: number) => {
        void electronAPI.config.write({ vuPanelHeight: height });
    }, []);

    const patchCodeRef = useRef(patchCode);
    useEffect(() => {
        patchCodeRef.current = patchCode;
    }, [patchCode]);

    // Stable per-buffer identity (the tab's id) used as the patch source id.
    // Unlike activeBufferId — a file's mutable path — this survives rename and
    // save, so reconciliation/clock-reset key on the buffer, not its path.
    const activeSourceId = useMemo(
        () =>
            buffers.find((b) => getBufferId(b) === activeBufferId)?.id ??
            activeBufferId,
        [buffers, activeBufferId],
    );
    const activeSourceIdRef = useRef(activeSourceId);
    useEffect(() => {
        activeSourceIdRef.current = activeSourceId;
    }, [activeSourceId]);

    // Keep runningBufferId pointing at the running buffer's current path
    // identity: saving an untitled buffer or renaming a file changes
    // getBufferId, and comparisons against activeBufferId (slider source
    // rewriting, running indicators) must keep matching afterwards.
    useEffect(() => {
        const sourceId = runningSourceIdRef.current;
        if (sourceId === null) {
            return;
        }
        const running = buffers.find((b) => b.id === sourceId);
        if (running) {
            setRunningBufferId(getBufferId(running));
        }
    }, [buffers]);

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
                // Hidden panel costs zero IPC.
                isVuPanelVisibleRef.current
                    ? electronAPI.synthesizer.getVuMeters()
                    : Promise.resolve([]),
            ])
                .then(([scopeData, transport, vuFrames]) => {
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

                    // Draw VU meters from this frame's levels. An empty poll
                    // is a lost try_lock race against the audio thread's
                    // drain block, not silence — keep the previous frame on
                    // screen instead of blinking the bars to the floor.
                    if (
                        isVuPanelVisibleRef.current &&
                        vuCanvasMapRef.current.size > 0 &&
                        vuFrames.length > 0
                    ) {
                        const frameByModule = new Map(
                            vuFrames.map((f) => [f.moduleId, f]),
                        );
                        const outputByKey = new Map(
                            vuOutputsRef.current.map((o) => [o.key, o]),
                        );
                        const vuColors = readVuMeterColors();
                        const now = performance.now();
                        for (const [key, canvas] of vuCanvasMapRef.current) {
                            const channelCount = Number(
                                canvas.dataset.channels ?? 1,
                            );
                            const frame = frameByModule.get(
                                canvas.dataset.tapModuleId ?? '',
                            );
                            if (!frame) {
                                continue;
                            }
                            let ballistics = vuBallisticsRef.current.get(key);
                            if (
                                !ballistics ||
                                ballistics.length !== channelCount
                            ) {
                                ballistics = Array.from(
                                    { length: channelCount },
                                    newBallistics,
                                );
                                vuBallisticsRef.current.set(key, ballistics);
                            }
                            const channels = ballistics.map((b, ch) => {
                                const rmsDb = voltsToDb(frame.rms[ch] ?? 0);
                                const peakDb = voltsToDb(frame.peak[ch] ?? 0);
                                updateBallistics(b, peakDb, now);
                                return {
                                    fastDb: b.displayFastDb,
                                    peakDb: b.displayPeakDb,
                                    rmsDb,
                                };
                            });
                            vuLastChannelsRef.current.set(key, channels);
                            const output = outputByKey.get(key);
                            // Signal-driven gains take the live value the
                            // engine sampled; editable ones use the state.
                            const gainDb =
                                frame.gain != null
                                    ? outGainToDb(frame.gain)
                                    : output && output.gain !== null
                                      ? outGainToDb(output.gain)
                                      : null;
                            const ghostGain =
                                vuGhostsRef.current.get(key)?.gain;
                            drawVuMeter(
                                canvas,
                                channels,
                                vuColors,
                                gainDb,
                                output?.gainLocked === true,
                                ghostGain !== undefined
                                    ? outGainToDb(ghostGain)
                                    : null,
                            );
                            if (frame.pan != null) {
                                const pointer =
                                    vuPanPointerMapRef.current.get(key);
                                pointer?.setAttribute(
                                    'transform',
                                    `rotate(${(Math.max(-5, Math.min(5, frame.pan)) / 5) * 135} 13 17)`,
                                );
                            }
                            const readout =
                                vuReadoutMapRef.current.get(key);
                            if (readout) {
                                readout.textContent = formatDb(
                                    Math.max(
                                        ...channels.map((c) => c.peakDb),
                                    ),
                                );
                            }
                        }
                    }

                    setTransport(transport);

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
                        vuDecorationsRef.current?.clear();
                        vuDecorationsRef.current = pending.vuDecorations;
                        setScopeViews(pending.scopeViews);
                        setSliderDefs(pending.sliderDefs);
                        setVuOutputs(pending.vuOutputs);
                        // The applied patch was compiled from the edited
                        // source, so audio and code agree again.
                        clearVuGhosts();
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
    }, [isClockRunning, clearVuGhosts]);

    // Keep Link phase indicator live while Link is enabled but Operator is stopped.
    // The main tick loop only runs when isClockRunning; this fills the gap so
    // the phase indicator stays animated even before the user presses play.
    useEffect(() => {
        if (!linkEnabled || isClockRunning) return;
        let cancelled = false;
        let rafId = 0;
        const tick = () => {
            if (cancelled) return;
            void electronAPI.synthesizer.getTransportState().then((t) => {
                if (cancelled) return;
                setTransport(t);
                rafId = requestAnimationFrame(tick);
            });
        };
        rafId = requestAnimationFrame(tick);
        return () => {
            cancelled = true;
            cancelAnimationFrame(rafId);
        };
    }, [linkEnabled, isClockRunning]);

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

                // Execute DSL in main process (has direct N-API access).
                // Use the stable buffer id (not activeBufferId, a file's mutable
                // path) so reconciliation/clock-reset key on the buffer itself.
                const result = await electronAPI.executeDSL(
                    patchCodeValue,
                    activeSourceIdRef.current,
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
                runningSourceIdRef.current = activeSourceIdRef.current ?? null;
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

                    // A scope whose call site cannot be resolved (source
                    // edited during the async round-trip) gets no decoration
                    // and a null decorationIndex: its zone is hidden.
                    const spanKey = loc ? `${loc.line}:${loc.column}` : '';
                    const range =
                        model && loc
                            ? resolveScopeCallRange(
                                  model,
                                  loc,
                                  callSiteSpans?.[spanKey],
                              )
                            : null;

                    views.push({
                        channelKeys,
                        decorationIndex: range ? decorationDescs.length : null,
                        file: activeBufferId,
                        key: scopeKey,
                        range: scope.range ?? [-5, 5],
                    });

                    if (range) {
                        decorationDescs.push({
                            options: {
                                stickiness:
                                    editor.TrackedRangeStickiness
                                        .NeverGrowsWhenTypingAtEdges,
                            },
                            range,
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

                // Track each out call's expression span so the VU meter M/S
                // buttons can edit the source later even after typing has
                // shifted it. Decoration index i belongs to newVuOutputs[i];
                // location-less entries get a degenerate never-edited range
                // to keep the indices aligned.
                const newVuOutputs = (result.appliedPatch?.vuMeters ??
                    []) as VuMeterDef[];
                const vuDecorationDescs: editor.IModelDeltaDecoration[] = [];
                for (const vu of newVuOutputs) {
                    const loc = vu.sourceLocation;
                    if (model && loc) {
                        const spanKey = `${loc.line}:${loc.column}`;
                        const callSpan = callSiteSpans?.[spanKey];
                        const endLine = callSpan?.endLine ?? loc.line;
                        const endLineContent =
                            model.getLineContent(endLine) ?? '';
                        // Captured columns are V8 columns: shifted by the
                        // executor wrapper's indent on line 1 only. The edit
                        // anchor must sit exactly on the method name.
                        const startColumn =
                            loc.line === 1
                                ? loc.column - FIRST_LINE_COLUMN_OFFSET
                                : loc.column;
                        vuDecorationDescs.push({
                            options: {
                                stickiness:
                                    editor.TrackedRangeStickiness
                                        .NeverGrowsWhenTypingAtEdges,
                            },
                            range: {
                                endColumn: endLineContent.length + 1,
                                endLineNumber: endLine,
                                startColumn,
                                startLineNumber: loc.line,
                            },
                        });
                    } else {
                        vuDecorationDescs.push({
                            options: {
                                stickiness:
                                    editor.TrackedRangeStickiness
                                        .NeverGrowsWhenTypingAtEdges,
                            },
                            range: {
                                endColumn: 1,
                                endLineNumber: 1,
                                startColumn: 1,
                                startLineNumber: 1,
                            },
                        });
                    }
                }
                let newVuDecorations: editor.IEditorDecorationsCollection | null =
                    null;
                if (editorInstance && vuDecorationDescs.length > 0) {
                    newVuDecorations =
                        editorInstance.createDecorationsCollection(
                            vuDecorationDescs,
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
                    pendingUIStateRef.current?.vuDecorations?.clear();
                    pendingUIStateRef.current = {
                        interpolationResolutions: interpolationMap,
                        scopeDecorations: newScopeDecorations,
                        scopeViews: views,
                        sliderDefs: newSliderDefs,
                        updateId: result.updateId,
                        vuDecorations: newVuDecorations,
                        vuOutputs: newVuOutputs,
                    };
                } else {
                    // Immediate trigger (or button click): swap decorations
                    // And apply UI state right away.
                    pendingUIStateRef.current?.scopeDecorations?.clear();
                    pendingUIStateRef.current?.vuDecorations?.clear();
                    pendingUIStateRef.current = null;
                    scopeDecorationsRef.current?.clear();
                    scopeDecorationsRef.current = newScopeDecorations;
                    vuDecorationsRef.current?.clear();
                    vuDecorationsRef.current = newVuDecorations;
                    setScopeViews(views);
                    setSliderDefs(newSliderDefs);
                    setVuOutputs(newVuOutputs);
                    // The applied patch was compiled from the edited source,
                    // so audio and code agree again.
                    clearVuGhosts();
                    if (interpolationMap) {
                        setActiveInterpolationResolutions(interpolationMap);
                    }
                }
            } catch (err) {
                setError(getErrorMessage(err, 'Unknown error'));
                setValidationErrors(null);
            }
        };
    }, [activeBufferId, clearVuGhosts]);

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
            getVuMeterData: () => electronAPI.synthesizer.getVuMeters(),
            getVuOutputs: () => vuOutputsRef.current,
            isClockRunning: () => isClockRunningRef.current,
            newUntitledFile: () => executeCommand('operator.newFile'),
            openEngineHealth: () => setIsEngineHealthOpen(true),
            openModuleProfile: () => setIsModuleProfileOpen(true),
            setEditorValue: (code: string) => editorRef.current?.setValue(code),
            setVuPanelVisible: (visible: boolean) =>
                setIsVuPanelVisible(visible),
            toggleVuMute: (key: string, codeOnly = false) =>
                handleVuToggle(key, 'mute', codeOnly),
            toggleVuSolo: (key: string, codeOnly = false) =>
                handleVuToggle(key, 'solo', codeOnly),
        };
        return () => {
            delete window.__TEST_API__;
        };
    }, [handleVuToggle]);

    const handleStopRef = useRef(() => {});
    useEffect(() => {
        handleStopRef.current = async () => {
            await electronAPI.synthesizer.stop();
            setIsClockRunning(false);
            setRunningBufferId(null);
            runningSourceIdRef.current = null;
        };
    }, []);
    const handleStop = useCallback(() => handleStopRef.current(), []);

    const dismissError = useCallback(() => {
        setError(null);
        setValidationErrors(null);
    }, []);

    const handleCloseBuffer = useCallback(
        async (id: string) => {
            setMigrationState(null);
            await closeBuffer(id);
        },
        [closeBuffer],
    );

    const handleSelectBuffer = useCallback(
        (id: string) => {
            setMigrationState(null);
            setActiveBufferId(id);
        },
        [setActiveBufferId],
    );

    useEffect(() => {
        setMigrationState((prev) =>
            prev && prev.bufferId !== activeBufferId ? null : prev,
        );
    }, [activeBufferId]);

    // Refs keep command handlers stable across renders so the registration
    // effect below only needs to run once. Anything captured by closure must
    // funnel through a ref or it goes stale between renders.
    const activeBufferIdRef = useRef(activeBufferId);
    useEffect(() => {
        activeBufferIdRef.current = activeBufferId;
    }, [activeBufferId]);

    const handleCloseBufferRef = useRef(handleCloseBuffer);
    useEffect(() => {
        handleCloseBufferRef.current = handleCloseBuffer;
    }, [handleCloseBuffer]);

    const createUntitledFileRef = useRef(createUntitledFile);
    useEffect(() => {
        createUntitledFileRef.current = createUntitledFile;
    }, [createUntitledFile]);

    // Ensure the keybindings.json exists (seeding a template if needed) and
    // open it as a normal editor buffer; saving it reloads the keymap live.
    const handleOpenKeybindings = useCallback(async () => {
        try {
            const path = await electronAPI.keybindings.ensureFile();
            keybindingsPathRef.current = path;
            await openAbsoluteFile(path);
        } catch (err) {
            setError(getErrorMessage(err, 'Failed to open keybindings'));
        }
    }, [openAbsoluteFile]);
    const handleOpenKeybindingsRef = useRef(handleOpenKeybindings);
    useEffect(() => {
        handleOpenKeybindingsRef.current = handleOpenKeybindings;
    }, [handleOpenKeybindings]);

    // Resolve the keybindings.json path up front so saves to it (even a
    // session-restored buffer) are recognized and trigger a keymap reload.
    useEffect(() => {
        electronAPI.keybindings
            .getPath()
            .then((path) => {
                keybindingsPathRef.current = path;
            })
            .catch(() => {});
    }, []);

    // Register operator.* commands in the global registry. The Electron menu
    // IPC dispatchers below, the cmdk palette, the editor context menu, and
    // the tinykeys keymap all dispatch through `executeCommand` — single
    // source of truth for what each command does.
    useEffect(() => {
        registerCommand(
            'operator.updatePatch',
            () => {
                handleSubmitRef.current('NextBar');
            },
            {
                label: 'Update Patch',
                category: 'Patch',
                contextMenu: { group: '1_patch', order: 1 },
            },
        );
        registerCommand(
            'operator.updatePatchNextBeat',
            () => {
                handleSubmitRef.current('NextBeat');
            },
            {
                label: 'Update Patch (Next Beat)',
                category: 'Patch',
                contextMenu: { group: '1_patch', order: 2 },
            },
        );
        registerCommand(
            'operator.stop',
            () => {
                handleStopRef.current();
            },
            {
                label: 'Stop',
                category: 'Patch',
                contextMenu: { group: '1_patch', order: 3 },
            },
        );
        registerCommand(
            'operator.newFile',
            () => {
                createUntitledFileRef.current();
            },
            { label: 'New File', category: 'File' },
        );
        registerCommand(
            'operator.closeBuffer',
            () => {
                const id = activeBufferIdRef.current;
                if (id) {
                    void handleCloseBufferRef.current(id);
                }
            },
            { label: 'Close Buffer', category: 'File' },
        );
        registerCommand(
            'operator.save',
            () => {
                handleSaveFileRef.current();
            },
            { label: 'Save', category: 'File' },
        );
        registerCommand(
            'operator.openWorkspace',
            () => {
                handleOpenWorkspaceRef.current();
            },
            { label: 'Open Workspace…', category: 'File' },
        );
        registerCommand(
            'operator.openSettings',
            () => {
                setIsSettingsOpen(true);
            },
            { label: 'Open Settings', category: 'Preferences' },
        );
        registerCommand(
            'operator.showCommandPalette',
            () => {
                setIsPaletteOpen(true);
            },
            {
                label: 'Show Command Palette',
                category: 'View',
                contextMenu: { group: 'z_commands', order: 1 },
            },
        );
        registerCommand(
            'operator.openKeybindings',
            () => {
                void handleOpenKeybindingsRef.current();
            },
            {
                label: 'Open Keyboard Shortcuts (JSON)',
                category: 'Preferences',
            },
        );
        registerCommand(
            'operator.toggleVuMeters',
            () => {
                setIsVuPanelVisible((visible) => {
                    void electronAPI.config.write({
                        vuPanelVisible: !visible,
                    });
                    return !visible;
                });
            },
            { label: 'Toggle VU Meters', category: 'View' },
        );

        return () => {
            unregisterCommand('operator.toggleVuMeters');
            unregisterCommand('operator.updatePatch');
            unregisterCommand('operator.updatePatchNextBeat');
            unregisterCommand('operator.stop');
            unregisterCommand('operator.newFile');
            unregisterCommand('operator.closeBuffer');
            unregisterCommand('operator.save');
            unregisterCommand('operator.openWorkspace');
            unregisterCommand('operator.openSettings');
            unregisterCommand('operator.showCommandPalette');
            unregisterCommand('operator.openKeybindings');
        };
    }, []);

    // Register the macOS-only "Toggle Syphon Output" command when supported,
    // so it shows in the palette and is bindable. The action lives in the main
    // process (SyphonBridge), so the handler round-trips through IPC. Gating on
    // support keeps it out of the palette where the menu item is also hidden.
    useEffect(() => {
        let active = true;
        void electronAPI.syphon.isSupported().then((supported) => {
            if (!active || !supported) return;
            registerCommand(
                'operator.toggleSyphon',
                () => {
                    void electronAPI.syphon.toggle();
                },
                { label: 'Toggle Syphon Output', category: 'View' },
            );
        });
        return () => {
            active = false;
            unregisterCommand('operator.toggleSyphon');
        };
    }, []);

    useEffect(() => {
        // Electron menu items dispatch through the registry so each handler
        // body lives in exactly one place. Recording is not in the registry —
        // toggling it reads render-state isRecording directly, hence this
        // effect's dependency on it.
        const cleanupNewFile = electronAPI.onMenuNewFile(() => {
            executeCommand('operator.newFile');
        });
        const cleanupSave = electronAPI.onMenuSave(() => {
            executeCommand('operator.save');
        });
        const cleanupStop = electronAPI.onMenuStop(() => {
            executeCommand('operator.stop');
        });
        const cleanupUpdate = electronAPI.onMenuUpdatePatch(() => {
            executeCommand('operator.updatePatch');
        });
        const cleanupUpdateNextBeat = electronAPI.onMenuUpdatePatchNextBeat(
            () => {
                executeCommand('operator.updatePatchNextBeat');
            },
        );
        const cleanupOpenWorkspace = electronAPI.onMenuOpenWorkspace(() => {
            executeCommand('operator.openWorkspace');
        });
        const cleanupCloseBuffer = electronAPI.onMenuCloseBuffer(() => {
            executeCommand('operator.closeBuffer');
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

        const cleanupToggleVuMeters = electronAPI.onMenuToggleVuMeters(() => {
            executeCommand('operator.toggleVuMeters');
        });

        // Handle opening settings from menu (Cmd+,)
        const cleanupOpenSettings = electronAPI.onMenuOpenSettings(() => {
            executeCommand('operator.openSettings');
        });
        const cleanupOpenEngineHealth = electronAPI.onMenuOpenEngineHealth(
            () => {
                setIsEngineHealthOpen(true);
            },
        );
        const cleanupOpenModuleProfile = electronAPI.onMenuOpenModuleProfile(
            () => {
                setIsModuleProfileOpen(true);
            },
        );
        const cleanupMigrateBuffer = electronAPI.onMenuMigrateBuffer(() => {
            const ed = editorRef.current;
            // Read the live id from the ref: this listener is registered once
            // (deps below), so closing over the `activeBufferId` state would
            // migrate a stale buffer after the user switches buffers.
            const activeId = activeBufferIdRef.current;
            if (!ed || !activeId) {
                console.warn('Migrate buffer: no editor available');
                setMigrationState({
                    bufferId: activeId ?? '',
                    original: '',
                    migrated: '',
                    summary: {
                        callsChanged: 0,
                        assignmentsChanged: 0,
                        commentsChanged: 0,
                        skippedVariables: [],
                        error: 'No editor available',
                    },
                });
                return;
            }
            const original = ed.getValue();
            const result = migrateCycleCalls(original);
            setMigrationState({
                bufferId: activeId,
                original,
                migrated: result.migrated,
                summary: {
                    callsChanged: result.callsChanged,
                    assignmentsChanged: result.assignmentsChanged,
                    commentsChanged: result.commentsChanged,
                    skippedVariables: result.skippedVariables,
                    error: result.error,
                },
            });
        });

        const cleanupMigrateWavetable = electronAPI.onMenuMigrateWavetable(
            () => {
                const ed = editorRef.current;
                // Read the live id from the ref: this listener is registered
                // once (deps below), so closing over the `activeBufferId` state
                // would migrate a stale buffer after the user switches buffers.
                const activeId = activeBufferIdRef.current;
                const wavetableTitle =
                    'Migrate $wavetable to pitch-first order';
                if (!ed || !activeId) {
                    console.warn('Migrate wavetable: no editor available');
                    setMigrationState({
                        bufferId: activeId ?? '',
                        original: '',
                        migrated: '',
                        title: wavetableTitle,
                        summary: {
                            callsChanged: 0,
                            commentsChanged: 0,
                            skippedVariables: [],
                            error: 'No editor available',
                        },
                    });
                    return;
                }
                const original = ed.getValue();
                const result = migrateWavetableArgs(original);
                setMigrationState({
                    bufferId: activeId,
                    original,
                    migrated: result.migrated,
                    title: wavetableTitle,
                    skippedLabel: 'Needs manual review:',
                    summary: {
                        callsChanged: result.callsChanged,
                        commentsChanged: result.commentsChanged,
                        skippedVariables: result.skipped,
                        error: result.error,
                    },
                });
            },
        );

        const cleanupMigrateChebyBlockDC =
            electronAPI.onMenuMigrateChebyBlockDC(() => {
                const ed = editorRef.current;
                // Read the live id from the ref: this listener is registered
                // once (deps below), so closing over the `activeBufferId` state
                // would migrate a stale buffer after the user switches buffers.
                const activeId = activeBufferIdRef.current;
                const chebyTitle =
                    'Migrate $cheby to preserve pre-DC-blocker output';
                if (!ed || !activeId) {
                    console.warn('Migrate $cheby: no editor available');
                    setMigrationState({
                        bufferId: activeId ?? '',
                        original: '',
                        migrated: '',
                        title: chebyTitle,
                        summary: {
                            callsChanged: 0,
                            commentsChanged: 0,
                            skippedVariables: [],
                            error: 'No editor available',
                        },
                    });
                    return;
                }
                const original = ed.getValue();
                const result = migrateChebyBlockDC(original);
                setMigrationState({
                    bufferId: activeId,
                    original,
                    migrated: result.migrated,
                    title: chebyTitle,
                    skippedLabel: 'Needs manual review:',
                    summary: {
                        callsChanged: result.callsChanged,
                        commentsChanged: 0,
                        skippedVariables: result.skipped,
                        error: result.error,
                    },
                });
            });

        return () => {
            cleanupNewFile();
            cleanupSave();
            cleanupStop();
            cleanupUpdate();
            cleanupUpdateNextBeat();
            cleanupOpenWorkspace();
            cleanupCloseBuffer();
            cleanupToggleRecording();
            cleanupToggleVuMeters();
            cleanupOpenSettings();
            cleanupOpenEngineHealth();
            cleanupOpenModuleProfile();
            cleanupMigrateBuffer();
            cleanupMigrateWavetable();
            cleanupMigrateChebyBlockDC();
        };
    }, [isRecording]);

    // Install the window-level tinykeys keymap. Runs after the operator.*
    // commands above have been registered (effects fire in declaration order),
    // so dispatch always finds a handler. The loader merges user overrides
    // from `<userData>/keybindings.json` on top of `DEFAULT_KEYMAP`.
    useEffect(() => {
        let disposed = false;
        let disposer: (() => void) | null = null;
        // Gate keybinding dispatch on the context-key service so VS Code-style
        // `when` clauses (editorTextFocus, etc.) are honored.
        setWhenEvaluator(evaluateWhen);
        const install = () => {
            loadAndInstallKeymap()
                .then((result) => {
                    if (disposed) {
                        result.dispose();
                        return;
                    }
                    // Drop the previous binding before swapping in the new one.
                    disposer?.();
                    disposer = result.dispose;
                })
                .catch((err) => {
                    console.error('[keymap] failed to install keymap:', err);
                });
        };
        install();
        // Re-read and re-install when the user saves keybindings.json.
        const onChanged = () => install();
        window.addEventListener('operator:keybindings-changed', onChanged);
        return () => {
            disposed = true;
            window.removeEventListener(
                'operator:keybindings-changed',
                onChanged,
            );
            disposer?.();
        };
    }, []);

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
                    onToggleLink={(enabled) => {
                        void electronAPI.synthesizer.enableLink(enabled);
                        // Optimistically update UI — polling only runs while playing
                        updateTransport((prev) => ({
                            ...prev,
                            linkEnabled: enabled,
                            linkPeers: enabled ? prev.linkPeers : 0,
                        }));
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

            <ModuleProfile
                isOpen={isModuleProfileOpen}
                onClose={() => setIsModuleProfileOpen(false)}
            />

            <AudioPanicDialog />

            <CommandPalette
                open={isPaletteOpen}
                onOpenChange={setIsPaletteOpen}
                editor={paletteEditor}
            />

            {migrationState && (
                <MigrationDiffModal
                    isOpen
                    original={migrationState.original}
                    migrated={migrationState.migrated}
                    summary={migrationState.summary}
                    title={migrationState.title}
                    skippedLabel={migrationState.skippedLabel}
                    onCancel={() => setMigrationState(null)}
                    onApply={() => {
                        const ed = editorRef.current;
                        const model = ed?.getModel();
                        if (!ed || !model) {
                            setMigrationState(null);
                            return;
                        }
                        if (migrationState.bufferId !== activeBufferId) {
                            console.warn(
                                'Migrate apply: active buffer changed since modal opened; aborting',
                            );
                            setMigrationState(null);
                            return;
                        }
                        model.pushEditOperations(
                            [],
                            [
                                {
                                    range: model.getFullModelRange(),
                                    text: migrationState.migrated,
                                },
                            ],
                            () => null,
                        );
                        setMigrationState(null);
                    }}
                />
            )}

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
                            {/* One shared canvas behind the editor. $scopeXY is
                                global, so every buffer would render identical
                                content; a single canvas (one WebGL context)
                                avoids exhausting the browser's context budget. */}
                            <ScopeXYBackground
                                intensity={xyScopeIntensity}
                                persistence={xyScopePersistence}
                                upsample={xyScopeUpsample}
                                lineWidth={xyScopeLineWidth}
                            />
                            <PatchEditor
                                value={patchCode}
                                runningBufferId={runningBufferId}
                                currentFile={activeBufferId}
                                onChange={handlePatchChange}
                                editorRef={editorRef}
                                onEditorChange={setPaletteEditor}
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
                                    onSelectBuffer={handleSelectBuffer}
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
            {isVuPanelVisible && workspaceRoot && (
                <VuMeterPanel
                    outputs={vuOutputs}
                    height={vuPanelHeight}
                    onHeightChange={setVuPanelHeight}
                    onHeightCommit={handleVuPanelHeightCommit}
                    onToggleMute={handleVuToggleMute}
                    onToggleSolo={handleVuToggleSolo}
                    ghosts={vuGhosts}
                    onPanChange={handleVuPanChange}
                    onPanCommit={handleVuPanCommit}
                    onGainChange={handleVuGainChange}
                    onGainCommit={handleVuGainCommit}
                    onGainReset={handleVuGainReset}
                    onGainRevert={handleVuGainRevert}
                    onPeakReset={handleVuPeakReset}
                    onCanvasResized={handleVuCanvasResized}
                    registerCanvas={registerVuCanvas}
                    unregisterCanvas={unregisterVuCanvas}
                    registerReadout={registerVuReadout}
                    unregisterReadout={unregisterVuReadout}
                    registerPanPointer={registerVuPanPointer}
                    unregisterPanPointer={unregisterVuPanPointer}
                />
            )}
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
