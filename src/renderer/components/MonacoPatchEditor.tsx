import React, { useEffect, useMemo, useRef, useState } from 'react';
import Editor, { type OnMount } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';
import { useTheme } from '../themes/ThemeContext';
import { useCustomMonaco } from '../hooks/useCustomMonaco';
import { configSchema } from '../configSchema';
import { formatPath } from './monaco/monacoHelpers';
import type { ScopeView } from '../types/editor';
import { setupMonacoJavascript } from './monaco/monacoLanguage';
import { buildSymbolSets } from './monaco/definitionProvider';
import {
    DEFAULT_PRETTIER_OPTIONS,
    registerDslFormattingProvider,
} from './monaco/formattingProvider';
import { applyMonacoTheme } from './monaco/theme';
import {
    registerConfigSchema,
    registerConfigSchemaForFile,
} from './monaco/jsonSchema';
import {
    type ScopeViewZoneHandle,
    createScopeViewZones,
} from './monaco/scopeViewZones';
import { startModuleStatePolling } from './monaco/moduleStateTracking';
import { registerMidiCompletionProvider } from './monaco/midiCompletionProvider';
import {
    bindEditorFocus,
    bindEditorWidgetVisibility,
} from '../keybindings/contextKeyBootstrap';
import electronAPI from '../electronAPI';
import type { Schemas } from '../../shared/dsl/schemaTypeResolver';
import { executeCommand } from '../keybindings/commands';

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
        return () => {
            onEditorChange?.(null);
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

    const handleMount: OnMount = (ed, _monacoInstance) => {
        setEditor(ed);
        editorRef.current = ed;

        // On Windows/Linux, use Ctrl; on macOS, use WinCtrl (physical Ctrl)
        // This ensures all shortcuts use the Control key on all platforms.
        const ctrl =
            electronAPI.platform === 'darwin'
                ? _monacoInstance.KeyMod.WinCtrl
                : _monacoInstance.KeyMod.CtrlCmd;

        // On Windows, Monaco swallows global accelerators, so we need to
        // Register them as Monaco keybindings that trigger the Electron menu actions.
        // Ctrl+Enter -> Update Patch (next bar; if already queued, Rust discards old + applies new immediately)
        ed.addCommand(ctrl | _monacoInstance.KeyCode.Enter, () => {
            window.electronAPI.triggerMenuAction('UPDATE_PATCH');
        });

        // Ctrl+Shift+Enter -> Update Patch (next beat)
        ed.addCommand(
            ctrl | _monacoInstance.KeyMod.Shift | _monacoInstance.KeyCode.Enter,
            () => {
                window.electronAPI.triggerMenuAction('UPDATE_PATCH_NEXT_BEAT');
            },
        );

        // Ctrl+. -> Stop Sound
        ed.addCommand(ctrl | _monacoInstance.KeyCode.Period, () => {
            window.electronAPI.triggerMenuAction('STOP');
        });

        // Ctrl+N -> New File
        ed.addCommand(ctrl | _monacoInstance.KeyCode.KeyN, () => {
            window.electronAPI.triggerMenuAction('NEW_FILE');
        });

        // Ctrl+W -> Close Buffer
        ed.addCommand(ctrl | _monacoInstance.KeyCode.KeyW, () => {
            window.electronAPI.triggerMenuAction('CLOSE_BUFFER');
        });

        // Override Monaco's built-in palette (F1, Ctrl+Shift+P) so both chords
        // open the Operator workbench palette instead of `editor.action.quickCommand`.
        // Outside the editor the same chord is caught by the window-level
        // keymap (Phase 2.3). Single palette, one keybinding, no conflict.
        ed.addCommand(_monacoInstance.KeyCode.F1, () => {
            executeCommand('operator.showCommandPalette');
        });
        ed.addCommand(
            ctrl | _monacoInstance.KeyMod.Shift | _monacoInstance.KeyCode.KeyP,
            () => {
                executeCommand('operator.showCommandPalette');
            },
        );
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
    // context-key service so when-clauses (Phase 2.x) can react.
    useEffect(() => {
        if (!editor) return;
        const stopFocus = bindEditorFocus(editor);
        const stopWidgets = bindEditorWidgetVisibility(editor);
        return () => {
            stopFocus();
            stopWidgets();
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
        const { moduleNames: _moduleNames, namespaceNames: _namespaceNames } =
            buildSymbolSets(schemas);
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

            // Console.log({ model, e });

            // Const match = resolveDslSymbolAtPosition(
            //     Model,
            //     E.target.position,
            //     ModuleNames,
            //     NamespaceNames,
            // );
            // If (match) {
            //     ElectronAPI.openHelpForSymbol(match.symbolType, match.symbolName);
            // }
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

    // Also configure schema when editing config file specifically
    useEffect(() => {
        if (!monaco || !currentFile?.endsWith('config.json')) {
            return;
        }
        registerConfigSchemaForFile(monaco, configSchema, currentFile);
    }, [monaco, currentFile]);

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
    // editor.updateOptions storm). Also: fixedOverflowWidgets lets the
    // suggest widget escape the .app-main { overflow: hidden } clipping
    // ancestor by reparenting to document.body.
    const editorOptions = useMemo<editor.IStandaloneEditorConstructionOptions>(
        () => ({
            minimap: { enabled: false },
            lineNumbers: 'on',
            folding: false,
            matchBrackets: 'always',
            automaticLayout: true,
            // Monaco's editor context menu hosts a "Command Palette"
            // entry that invokes `editor.action.quickCommand`
            // directly, bypassing our keybinding override. Disable
            // the built-in menu wholesale; Phase 2.1b replaces it
            // with a Radix context menu.
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
