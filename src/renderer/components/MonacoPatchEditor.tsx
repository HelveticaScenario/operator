import React, { useEffect, useMemo, useRef, useState } from 'react';
import Editor, { type OnMount } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';
import { useTheme } from '../themes/ThemeContext';
import { useCustomMonaco } from '../hooks/useCustomMonaco';
import { configSchema } from '../configSchema';
import { formatPath } from './monaco/monacoHelpers';
import type { ScopeView } from '../types/editor';
import { setupMonacoJavascript } from './monaco/monacoLanguage';
import {
    DEFAULT_PRETTIER_OPTIONS,
    registerDslFormattingProvider,
} from './monaco/formattingProvider';
import { applyMonacoTheme } from './monaco/theme';
import { registerConfigSchema } from './monaco/jsonSchema';
import {
    type ScopeViewZoneHandle,
    createScopeViewZones,
} from './monaco/scopeViewZones';
import { startModuleStatePolling } from './monaco/moduleStateTracking';
import { registerMidiCompletionProvider } from './monaco/midiCompletionProvider';
import { registerKeybindingsCompletionProvider } from './monaco/keybindingsCompletion';
import {
    bindEditorContextConstants,
    bindEditorFocus,
    bindEditorWidgetVisibility,
} from '../keybindings/contextKeyBootstrap';
import electronAPI from '../electronAPI';
import type { Schemas } from '../../shared/dsl/schemaTypeResolver';
import { buildEditorMenuItems } from '../keybindings/editorMenuItems';
import { dispatchCommand, setActiveEditor } from '../keybindings/dispatch';

export interface PatchEditorProps {
    value: string;
    currentFile?: string;
    onChange: (value: string) => void;
    editorRef: React.RefObject<editor.IStandaloneCodeEditor | null>;
    /**
     * Notified when the editor instance becomes available (mount) or goes
     * away (unmount, e.g. last buffer closed). Lets parents subscribe via
     * state instead of reading `editorRef.current` during render.
     */
    onEditorChange?: (editor: editor.IStandaloneCodeEditor | null) => void;
    scopeViews?: ScopeView[];
    /** Tracked decoration collection whose ranges correspond 1:1 with scopeViews. */
    scopeDecorations?: editor.IEditorDecorationsCollection | null;
    onRegisterScopeCanvas?: (key: string, canvas: HTMLCanvasElement) => void;
    onUnregisterScopeCanvas?: (key: string) => void;
    runningBufferId?: string | null;
}

export function MonacoPatchEditor({
    value,
    currentFile,
    onChange,
    editorRef,
    onEditorChange,
    scopeViews = [],
    scopeDecorations = null,
    onRegisterScopeCanvas,
    onUnregisterScopeCanvas,
    runningBufferId,
}: PatchEditorProps) {
    // Fetch DSL lib source once at mount for Monaco autocomplete
    const [libSource, setLibSource] = useState<string | null>(null);
    const [schemas, setSchemas] = useState<Schemas>([]);

    useEffect(() => {
        electronAPI.getDslLibSource().then(setLibSource).catch(console.error);
        electronAPI.getSchemas().then(setSchemas).catch(console.error);
    }, []);

    // Re-fetch DSL lib source when wavs folder changes so Monaco picks up new $wavs() types
    useEffect(() => {
        const unsubscribe = electronAPI.onWavsChange(() => {
            electronAPI
                .getDslLibSource()
                .then(setLibSource)
                .catch(console.error);
        });
        return unsubscribe;
    }, []);

    const monaco = useCustomMonaco();
    const [editor, setEditor] = useState<editor.IStandaloneCodeEditor | null>(
        null,
    );

    // Mirror the local `editor` state up to the parent (for the command
    // palette etc.). Cleans up to `null` when the inner Monaco Editor
    // unmounts (e.g., when the last buffer closes and `currentFile` goes
    // falsy).
    useEffect(() => {
        onEditorChange?.(editor);
        // Make this editor the dispatch target for window-level keybindings
        // and the context menu (see keybindings/dispatch).
        setActiveEditor(editor);
        return () => {
            onEditorChange?.(null);
            setActiveEditor(null);
        };
    }, [editor, onEditorChange]);

    // Decoration collection for active module state highlighting (seq steps, etc.)
    const activeDecorationRef =
        useRef<editor.IEditorDecorationsCollection | null>(null);

    // Poll module states for active step highlighting using the generic system
    // This uses argument_spans from Rust to know where arguments are in the document,
    // Combined with source_spans for internal highlighting (like mini-notation spans)
    useEffect(() => {
        if (!editor || !monaco) {
            return;
        }
        return startModuleStatePolling({
            activeDecorationRef,
            currentFile,
            editor,
            getModuleStates: () =>
                window.electronAPI.synthesizer.getModuleStates(),
            monaco,
            runningBufferId,
        });
    }, [editor, monaco, currentFile, runningBufferId]);

    // Ref to hold the current scope view zone handle for repositioning
    const scopeZoneHandleRef = useRef<ScopeViewZoneHandle | null>(null);

    // Filter scope views to only those belonging to the active file
    const activeScopeViews = useMemo(
        () => scopeViews.filter((view) => view.file === currentFile),
        [scopeViews, currentFile],
    );

    // Create / recreate scope view zones when the scope list changes
    useEffect(() => {
        if (!editor || !monaco) {
            return;
        }
        const handle = createScopeViewZones({
            editor,
            monaco,
            onRegisterScopeCanvas,
            onUnregisterScopeCanvas,
            scopeDecorations,
            views: activeScopeViews,
        });
        scopeZoneHandleRef.current = handle;
        return () => {
            handle.dispose();
            scopeZoneHandleRef.current = null;
        };
    }, [
        editor,
        monaco,
        activeScopeViews,
        scopeDecorations,
        onRegisterScopeCanvas,
        onUnregisterScopeCanvas,
    ]);

    // On every content change, re-read positions from tracked decorations and
    // Reposition view zones if any scope call has moved to a different line.
    useEffect(() => {
        if (!editor) {
            return;
        }
        const disposable = editor.onDidChangeModelContent(() => {
            scopeZoneHandleRef.current?.repositionZones();
        });
        return () => disposable.dispose();
    }, [editor]);

    // Native editor context menu (right-click). Monaco's built-in menu is
    // disabled (it hosts a "Command Palette" entry that bypasses our keymap),
    // so we pop a native Electron menu built from the command registry and
    // dispatch the chosen item through the shared command path.
    useEffect(() => {
        if (!editor) {
            return;
        }
        const contextSub = editor.onContextMenu((e) => {
            // Focus on every right-click (editor convention) so the native
            // clipboard roles act on this editor.
            editor.focus();
            const position = e.target.position;
            const selection = editor.getSelection();
            // Right-clicking outside the current selection collapses the caret
            // to the click point so paste lands where the user clicked; an
            // existing selection is preserved so copy/cut act on it.
            if (
                position &&
                (!selection || !selection.containsPosition(position))
            ) {
                editor.setPosition(position);
            }
            void electronAPI.showContextMenu({
                type: 'editor',
                items: buildEditorMenuItems(editor),
            });
        });
        const disposeCommandSub = electronAPI.onContextMenuCommand((action) => {
            if (action.command !== 'editor' || !action.commandId) {
                return;
            }
            // Registry commands run via the registry; editor actions and core
            // editor commands are triggered on this editor. Clipboard roles
            // never round-trip — they are handled natively in the main process.
            // Refocus first: the native menu took focus, and editor commands
            // (Go to Definition, etc.) need the editor focused to act.
            editor.focus();
            dispatchCommand(action.commandId);
        });
        return () => {
            contextSub.dispose();
            disposeCommandSub();
        };
    }, [editor]);

    const handleMount: OnMount = (ed) => {
        setEditor(ed);
        editorRef.current = ed;
        // When the inner <Editor> unmounts (e.g. the last buffer closes and
        // currentFile goes falsy) Monaco disposes this editor. Drop our
        // references so the mirror effect below clears the parent's
        // paletteEditor and the dispatch target instead of leaving them
        // pointing at a disposed editor.
        ed.onDidDispose(() => {
            setEditor((current) => (current === ed ? null : current));
            if (editorRef.current === ed) {
                editorRef.current = null;
            }
        });
        // No editor-level keybindings are registered here: the capture-phase
        // window keymap (keybindings/keymap) owns every shortcut and runs
        // before Monaco, so it is the single source of truth. Hardcoding
        // editor bindings here would shadow the keymap and could not be
        // rebound or removed via keybindings.json.
    };

    useEffect(() => {
        if (!monaco || !libSource) {
            return;
        }
        return setupMonacoJavascript(monaco, libSource, {
            schemas,
        });
    }, [monaco, libSource, schemas]);

    // Mirror Monaco focus and suggest/find widget visibility into the
    // context-key service so when-clauses can react.
    useEffect(() => {
        if (!editor) return;
        const stopFocus = bindEditorFocus(editor);
        const stopWidgets = bindEditorWidgetVisibility(editor);
        const stopConstants = bindEditorContextConstants(editor);
        return () => {
            stopFocus();
            stopWidgets();
            stopConstants();
        };
    }, [editor]);

    const {
        theme: appTheme,
        cursorStyle,
        font,
        fontLigatures,
        fontSize,
        prettierConfig,
    } = useTheme();
    const monacoThemeId = `theme-${appTheme.id}`;

    // Open help for DSL symbols on Cmd+Click (not Cmd+Hover)
    useEffect(() => {
        if (!editor || !monaco || schemas.length === 0) {
            return;
        }
        const disposable = editor.onMouseDown((e) => {
            // Check for Cmd (Mac) / Ctrl (Win/Linux) + primary button click
            if (!e.event.metaKey && !e.event.ctrlKey) {
                return;
            }
            if (e.target.position == null) {
                return;
            }

            const model = editor.getModel();
            if (!model) {
                return;
            }

            editor.focus();
            editor.trigger('api', 'editor.action.peekDefinition', {});
        });
        return () => disposable.dispose();
    }, [editor, monaco, schemas]);

    useEffect(() => {
        if (!monaco) {
            return;
        }
        const disposable = registerDslFormattingProvider(
            monaco,
            prettierConfig,
        );
        return () => disposable.dispose();
    }, [monaco, prettierConfig]);

    useEffect(() => {
        if (!editor) {
            return;
        }
        const apply = () => {
            const model = editor.getModel();
            if (model) {
                model.updateOptions({
                    insertSpaces: true,
                    tabSize:
                        prettierConfig.tabWidth ??
                        DEFAULT_PRETTIER_OPTIONS.tabWidth,
                });
            }
        };
        apply();
        const disposable = editor.onDidChangeModel(apply);
        return () => disposable.dispose();
    }, [editor, prettierConfig.tabWidth]);

    // Register MIDI device autocomplete provider
    useEffect(() => {
        if (!monaco) {
            return;
        }
        const midiProvider = registerMidiCompletionProvider(monaco, () =>
            electronAPI.midi.listInputs(),
        );
        return () => midiProvider.dispose();
    }, [monaco]);

    // Autocomplete command ids and `when` context keys in the keybindings.json
    // buffer (provider scopes itself to that model).
    useEffect(() => {
        if (!monaco) {
            return;
        }
        const provider = registerKeybindingsCompletionProvider(monaco);
        return () => provider.dispose();
    }, [monaco]);

    // Define Monaco theme from the current app theme
    useEffect(() => {
        if (!monaco) {
            return;
        }
        applyMonacoTheme(monaco, appTheme, monacoThemeId);
    }, [monaco, appTheme, monacoThemeId]);

    // Configure JSON schema for config files
    useEffect(() => {
        if (!monaco) {
            return;
        }
        registerConfigSchema(monaco, configSchema);
    }, [monaco]);

    // Determine language based on file extension
    const editorLanguage = useMemo(() => {
        if (!currentFile) {
            return 'javascript';
        }
        if (currentFile.endsWith('.json')) {
            return 'json';
        }
        return 'javascript';
    }, [currentFile]);

    // Memoize options so the @monaco-editor/react wrapper's [options] effect
    // does not fire on every parent render (App.tsx re-renders at ~60Hz via
    // the scope-polling RAF loop, which would otherwise cause a constant
    // editor.updateOptions storm). Also: fixedOverflowWidgets renders the
    // overflowing widgets (suggest, hover, parameter hints) with
    // position: fixed so they escape the .app-main { overflow: hidden }
    // clipping ancestor. The widgets stay inside the editor's DOM node, so
    // contextKeyBootstrap's widget-visibility observer still sees them.
    const editorOptions = useMemo<editor.IStandaloneEditorConstructionOptions>(
        () => ({
            minimap: { enabled: false },
            // The transparent editor background lets the scrolled lines bleed
            // through a pinned sticky header as a ghosted duplicate, so the
            // sticky-scroll feature is disabled here.
            stickyScroll: { enabled: false },
            lineNumbers: 'on',
            folding: false,
            matchBrackets: 'always',
            automaticLayout: true,
            // Monaco's built-in menu hosts a "Command Palette" entry that
            // invokes `editor.action.quickCommand` directly, bypassing our
            // keymap. Disable it; the native right-click menu wired up in the
            // onContextMenu effect above replaces it.
            contextmenu: false,
            fontFamily: `${font}, monospace`,
            fontLigatures: fontLigatures,
            fontSize: fontSize,
            // LineHeight: 1.6,
            padding: { bottom: 8, top: 8 },
            renderLineHighlight: 'line',
            cursorBlinking: 'solid',
            cursorStyle: cursorStyle,
            scrollbar: {
                horizontal: 'auto',
                horizontalScrollbarSize: 8,
                vertical: 'auto',
                verticalScrollbarSize: 8,
            },
            overviewRulerBorder: false,
            hideCursorInOverviewRuler: true,
            renderLineHighlightOnlyWhenFocus: false,
            guides: {
                bracketPairs: false,
                indentation: true,
            },
            fixedOverflowWidgets: true,
        }),
        [font, fontLigatures, fontSize, cursorStyle],
    );

    return (
        <div className="patch-editor" style={{ height: '100%' }}>
            {currentFile && (
                <Editor
                    height="100%"
                    path={formatPath(currentFile)}
                    language={editorLanguage}
                    theme={monacoThemeId}
                    value={value}
                    onChange={(val) => {
                        onChange(val ?? '');
                    }}
                    onMount={handleMount}
                    keepCurrentModel
                    options={editorOptions}
                />
            )}
        </div>
    );
}
