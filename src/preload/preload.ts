// See the Electron documentation for details on how to use preload scripts:
// https://www.electronjs.org/docs/latest/tutorial/process-model#preload-scripts
import { contextBridge, ipcRenderer } from 'electron/renderer';
import {
    AppConfig,
    ContextMenuAction,
    ContextMenuOptions,
    DSLExecuteResult,
    IPCHandlers,
    IPCRequest,
    IPCResponse,
    IPC_CHANNELS,
    MENU_CHANNELS,
    MainLogEntry,
    Promisify,
    UpdateAvailableInfo,
} from '../shared/ipcTypes';
import type { QueuedTrigger } from '../shared/ipcTypes';

/**
 * Type-safe wrapper for IPC invoke calls
 */
function invokeIPC<T extends keyof typeof IPC_CHANNELS>(
    channel: T,
    ...args: IPCRequest<(typeof IPC_CHANNELS)[T]>
): IPCResponse<(typeof IPC_CHANNELS)[T]> {
    // @ts-expect-error - TypeScript is having trouble inferring the return type here
    return ipcRenderer.invoke(IPC_CHANNELS[channel], ...args);
}

/**
 * The public API exposed to the renderer process.
 * All methods are type-safe and match the @modular/core interface.
 */

export interface ElectronAPI {
    // Platform info (synchronous, available immediately)
    platform: NodeJS.Platform;

    // Schema operations
    getSchemas: Promisify<IPCHandlers[typeof IPC_CHANNELS.GET_SCHEMAS]>;
    getMiniLeafSpans: Promisify<
        IPCHandlers[typeof IPC_CHANNELS.GET_MINI_LEAF_SPANS]
    >;

    // DSL operations
    executeDSL: (
        source: string,
        sourceId?: string,
        trigger?: QueuedTrigger,
    ) => Promise<DSLExecuteResult>;
    getDslLibSource: () => Promise<string>;

    // Synthesizer operations
    synthesizer: {
        getSampleRate: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_GET_SAMPLE_RATE]
        >;
        getChannels: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_GET_CHANNELS]
        >;
        getScopes: Promisify<IPCHandlers[typeof IPC_CHANNELS.SYNTH_GET_SCOPES]>;
        getModuleStates: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_GET_MODULE_STATES]
        >;
        updatePatch: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_UPDATE_PATCH]
        >;
        startRecording: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_START_RECORDING]
        >;
        stopRecording: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_STOP_RECORDING]
        >;
        isRecording: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_IS_RECORDING]
        >;
        getHealth: Promisify<IPCHandlers[typeof IPC_CHANNELS.SYNTH_GET_HEALTH]>;
        stop: Promisify<IPCHandlers[typeof IPC_CHANNELS.SYNTH_STOP]>;
        isStopped: Promisify<IPCHandlers[typeof IPC_CHANNELS.SYNTH_IS_STOPPED]>;
        setModuleParam: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_SET_MODULE_PARAM]
        >;
        getTransportState: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_GET_TRANSPORT_STATE]
        >;
        enableLink: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_ENABLE_LINK]
        >;
        isAudioThreadPanicked: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_IS_AUDIO_THREAD_PANICKED]
        >;
        restartAudio: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_RESTART_AUDIO]
        >;
        panicLogDir: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.SYNTH_PANIC_LOG_DIR]
        >;
    };
    // Audio device operations
    audio: {
        // New API
        refreshDeviceCache: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_REFRESH_DEVICE_CACHE]
        >;
        getDeviceCache: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_GET_DEVICE_CACHE]
        >;
        getCurrentState: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_GET_CURRENT_STATE]
        >;
        recreateStreams: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_RECREATE_STREAMS]
        >;
        onFallbackWarning: (callback: (warning: string) => void) => () => void;
        // Legacy (kept for backward compatibility)
        refreshDeviceList: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_REFRESH_DEVICE_LIST]
        >;
        listHosts: Promisify<IPCHandlers[typeof IPC_CHANNELS.AUDIO_LIST_HOSTS]>;
        listOutputDevices: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_LIST_OUTPUT_DEVICES]
        >;
        listInputDevices: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_LIST_INPUT_DEVICES]
        >;
        getOutputDevice: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_GET_OUTPUT_DEVICE]
        >;
        getInputDevice: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_GET_INPUT_DEVICE]
        >;
        setOutputDevice: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_SET_OUTPUT_DEVICE]
        >;
        setInputDevice: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_SET_INPUT_DEVICE]
        >;
        getInputChannels: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.AUDIO_GET_INPUT_CHANNELS]
        >;
    };
    // MIDI device operations
    midi: {
        listInputs: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.MIDI_LIST_INPUTS]
        >;
        getInput: Promisify<IPCHandlers[typeof IPC_CHANNELS.MIDI_GET_INPUT]>;
        setInput: Promisify<IPCHandlers[typeof IPC_CHANNELS.MIDI_SET_INPUT]>;
        tryReconnect: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.MIDI_TRY_RECONNECT]
        >;
    };
    // Filesystem operations
    filesystem: {
        selectWorkspace: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.FS_SELECT_WORKSPACE]
        >;
        getWorkspace: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.FS_GET_WORKSPACE]
        >;
        listFiles: Promisify<IPCHandlers[typeof IPC_CHANNELS.FS_LIST_FILES]>;
        readFile: Promisify<IPCHandlers[typeof IPC_CHANNELS.FS_READ_FILE]>;
        writeFile: Promisify<IPCHandlers[typeof IPC_CHANNELS.FS_WRITE_FILE]>;
        renameFile: Promisify<IPCHandlers[typeof IPC_CHANNELS.FS_RENAME_FILE]>;
        deleteFile: Promisify<IPCHandlers[typeof IPC_CHANNELS.FS_DELETE_FILE]>;
        moveFile: Promisify<IPCHandlers[typeof IPC_CHANNELS.FS_MOVE_FILE]>;
        createFolder: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.FS_CREATE_FOLDER]
        >;
        showSaveDialog: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.FS_SHOW_SAVE_DIALOG]
        >;
        showInputDialog: Promisify<
            IPCHandlers[typeof IPC_CHANNELS.FS_SHOW_INPUT_DIALOG]
        >;
    };
    // Menu events
    onMenuNewFile: (callback: () => void) => () => void;
    onMenuSave: (callback: () => void) => () => void;
    onMenuStop: (callback: () => void) => () => void;
    onMenuUpdatePatch: (
        callback: (trigger?: QueuedTrigger) => void,
    ) => () => void;
    onMenuUpdatePatchNextBeat: (callback: () => void) => () => void;
    onMenuOpenWorkspace: (callback: () => void) => () => void;
    onMenuCloseBuffer: (callback: () => void) => () => void;
    onMenuToggleRecording: (callback: () => void) => () => void;
    onMenuOpenSettings: (callback: () => void) => () => void;
    onMenuOpenEngineHealth: (callback: () => void) => () => void;
    /**
     * Trigger a menu action programmatically (e.g., from Monaco keybindings).
     * This emits the same IPC event that the Electron menu would send.
     */
    triggerMenuAction: (action: keyof typeof MENU_CHANNELS) => void;
    // UI operations
    showContextMenu: (options: ContextMenuOptions) => Promise<void>;
    onContextMenuCommand: (
        callback: (action: ContextMenuAction) => void,
    ) => () => void;
    showUnsavedChangesDialog: (fileName: string) => Promise<number>;

    // Window operations
    openHelpWindow: () => Promise<void>;
    openHelpForSymbol: (
        symbolType: 'type' | 'module' | 'namespace',
        symbolName: string,
    ) => Promise<void>;
    onNavigateToSymbol: (
        callback: (data: {
            symbolType: 'type' | 'module' | 'namespace';
            symbolName: string;
        }) => void,
    ) => () => void;

    // Config operations
    config: {
        getPath: () => Promise<string>;
        read: () => Promise<AppConfig>;
        write: (config: Partial<AppConfig>) => Promise<void>;
        onChange: (callback: (config: AppConfig) => void) => () => void;
    };

    // Wavs folder change notification
    onWavsChange: (callback: () => void) => () => void;

    // Main process log forwarding
    onMainLog: (callback: (entry: MainLogEntry) => void) => () => void;

    // OS shell operations
    shell: {
        openPath: Promisify<IPCHandlers[typeof IPC_CHANNELS.SHELL_OPEN_PATH]>;
    };

    // Update operations
    update: {
        check: () => Promise<void>;
        download: () => Promise<void>;
        install: () => Promise<void>;
        onAvailable: (
            callback: (info: UpdateAvailableInfo) => void,
        ) => () => void;
        onDownloading: (callback: () => void) => () => void;
        onDownloaded: (callback: () => void) => () => void;
        onError: (callback: (message: string) => void) => () => void;
    };
}

const electronAPI: ElectronAPI = {
    // Platform info
    platform: process.platform,

    // Schema operations
    getSchemas: (...args) => invokeIPC('GET_SCHEMAS', ...args),
    getMiniLeafSpans: (...args) => invokeIPC('GET_MINI_LEAF_SPANS', ...args),

    // DSL operations
    executeDSL: (source, sourceId, trigger) =>
        invokeIPC('DSL_EXECUTE', source, sourceId, trigger),
    getDslLibSource: () => invokeIPC('GET_DSL_LIB_SOURCE'),

    // Window operations
    openHelpWindow: () => invokeIPC('OPEN_HELP_WINDOW'),
    openHelpForSymbol: (
        symbolType: 'type' | 'module' | 'namespace',
        symbolName: string,
    ) => invokeIPC('OPEN_HELP_FOR_SYMBOL', symbolType, symbolName),
    onNavigateToSymbol: (
        callback: (data: {
            symbolType: 'type' | 'module' | 'namespace';
            symbolName: string;
        }) => void,
    ) => {
        const listener = (
            _event: Electron.IpcRendererEvent,
            data: {
                symbolType: 'type' | 'module' | 'namespace';
                symbolName: string;
            },
        ) => callback(data);
        ipcRenderer.on('navigate-to-symbol', listener);
        return () => ipcRenderer.removeListener('navigate-to-symbol', listener);
    },

    // Synthesizer operations
    synthesizer: {
        getChannels: (...args) => invokeIPC('SYNTH_GET_CHANNELS', ...args),

        getHealth: (...args) => invokeIPC('SYNTH_GET_HEALTH', ...args),

        getModuleStates: (...args) =>
            invokeIPC('SYNTH_GET_MODULE_STATES', ...args),

        getSampleRate: (...args) => invokeIPC('SYNTH_GET_SAMPLE_RATE', ...args),

        getScopes: (...args) => invokeIPC('SYNTH_GET_SCOPES', ...args),

        getTransportState: (...args) =>
            invokeIPC('SYNTH_GET_TRANSPORT_STATE', ...args),

        enableLink: (...args) => invokeIPC('SYNTH_ENABLE_LINK', ...args),

        isAudioThreadPanicked: (...args) =>
            invokeIPC('SYNTH_IS_AUDIO_THREAD_PANICKED', ...args),

        restartAudio: (...args) => invokeIPC('SYNTH_RESTART_AUDIO', ...args),

        panicLogDir: (...args) => invokeIPC('SYNTH_PANIC_LOG_DIR', ...args),

        isRecording: (...args) => invokeIPC('SYNTH_IS_RECORDING', ...args),

        isStopped: (...args) => invokeIPC('SYNTH_IS_STOPPED', ...args),

        setModuleParam: (...args) =>
            invokeIPC('SYNTH_SET_MODULE_PARAM', ...args),

        startRecording: (...args) =>
            invokeIPC('SYNTH_START_RECORDING', ...args),

        stop: (...args) => invokeIPC('SYNTH_STOP', ...args),

        stopRecording: (...args) => invokeIPC('SYNTH_STOP_RECORDING', ...args),

        updatePatch: (...args) => invokeIPC('SYNTH_UPDATE_PATCH', ...args),
    },

    // Audio device operations
    audio: {
        // New API
        refreshDeviceCache: (...args) =>
            invokeIPC('AUDIO_REFRESH_DEVICE_CACHE', ...args),

        getDeviceCache: (...args) =>
            invokeIPC('AUDIO_GET_DEVICE_CACHE', ...args),

        getCurrentState: (...args) =>
            invokeIPC('AUDIO_GET_CURRENT_STATE', ...args),

        recreateStreams: (...args) =>
            invokeIPC('AUDIO_RECREATE_STREAMS', ...args),

        onFallbackWarning: menuEventHandler(
            IPC_CHANNELS.AUDIO_FALLBACK_WARNING,
        ),

        // Legacy (kept for backward compatibility)
        refreshDeviceList: (...args) =>
            invokeIPC('AUDIO_REFRESH_DEVICE_LIST', ...args),

        listHosts: (...args) => invokeIPC('AUDIO_LIST_HOSTS', ...args),

        listOutputDevices: (...args) =>
            invokeIPC('AUDIO_LIST_OUTPUT_DEVICES', ...args),

        listInputDevices: (...args) =>
            invokeIPC('AUDIO_LIST_INPUT_DEVICES', ...args),

        getOutputDevice: (...args) =>
            invokeIPC('AUDIO_GET_OUTPUT_DEVICE', ...args),

        getInputDevice: (...args) =>
            invokeIPC('AUDIO_GET_INPUT_DEVICE', ...args),

        setOutputDevice: (...args) =>
            invokeIPC('AUDIO_SET_OUTPUT_DEVICE', ...args),

        setInputDevice: (...args) =>
            invokeIPC('AUDIO_SET_INPUT_DEVICE', ...args),

        getInputChannels: (...args) =>
            invokeIPC('AUDIO_GET_INPUT_CHANNELS', ...args),
    },

    // MIDI device operations
    midi: {
        getInput: (...args) => invokeIPC('MIDI_GET_INPUT', ...args),

        listInputs: (...args) => invokeIPC('MIDI_LIST_INPUTS', ...args),

        setInput: (...args) => invokeIPC('MIDI_SET_INPUT', ...args),

        tryReconnect: (...args) => invokeIPC('MIDI_TRY_RECONNECT', ...args),
    },

    // Filesystem operations
    filesystem: {
        createFolder: (...args) => invokeIPC('FS_CREATE_FOLDER', ...args),

        deleteFile: (...args) => invokeIPC('FS_DELETE_FILE', ...args),

        getWorkspace: (...args) => invokeIPC('FS_GET_WORKSPACE', ...args),

        listFiles: (...args) => invokeIPC('FS_LIST_FILES', ...args),

        moveFile: (...args) => invokeIPC('FS_MOVE_FILE', ...args),

        readFile: (...args) => invokeIPC('FS_READ_FILE', ...args),

        renameFile: (...args) => invokeIPC('FS_RENAME_FILE', ...args),

        selectWorkspace: (...args) => invokeIPC('FS_SELECT_WORKSPACE', ...args),

        showInputDialog: (...args) =>
            invokeIPC('FS_SHOW_INPUT_DIALOG', ...args),

        showSaveDialog: (...args) => invokeIPC('FS_SHOW_SAVE_DIALOG', ...args),

        writeFile: (...args) => invokeIPC('FS_WRITE_FILE', ...args),
    },

    // Menu events
    onMenuNewFile: menuEventHandler(MENU_CHANNELS.NEW_FILE),
    onMenuSave: menuEventHandler(MENU_CHANNELS.SAVE),
    onMenuStop: menuEventHandler(MENU_CHANNELS.STOP),
    onMenuUpdatePatch: menuEventHandler(MENU_CHANNELS.UPDATE_PATCH),
    onMenuUpdatePatchNextBeat: menuEventHandler(
        MENU_CHANNELS.UPDATE_PATCH_NEXT_BEAT,
    ),
    onMenuOpenWorkspace: menuEventHandler(MENU_CHANNELS.OPEN_WORKSPACE),
    onMenuCloseBuffer: menuEventHandler(MENU_CHANNELS.CLOSE_BUFFER),
    onMenuToggleRecording: menuEventHandler(MENU_CHANNELS.TOGGLE_RECORDING),
    onMenuOpenSettings: menuEventHandler(MENU_CHANNELS.OPEN_SETTINGS),
    onMenuOpenEngineHealth: menuEventHandler(MENU_CHANNELS.OPEN_ENGINE_HEALTH),
    // Programmatically trigger a menu action (for Monaco keybindings on Windows)
    triggerMenuAction: (action: keyof typeof MENU_CHANNELS) => {
        const channel = MENU_CHANNELS[action];
        if (channel) {
            // Emit the event locally so registered listeners receive it
            ipcRenderer.emit(channel, { sender: ipcRenderer });
        }
    },

    // UI operations
    showContextMenu: (options) => invokeIPC('SHOW_CONTEXT_MENU', options),
    onContextMenuCommand: menuEventHandler(
        IPC_CHANNELS.ON_CONTEXT_MENU_COMMAND,
    ),
    showUnsavedChangesDialog: (fileName) =>
        invokeIPC('SHOW_UNSAVED_CHANGES_DIALOG', fileName),

    // Config operations
    config: {
        getPath: () => invokeIPC('CONFIG_GET_PATH'),
        onChange: menuEventHandler(IPC_CHANNELS.CONFIG_ON_CHANGE),
        read: () => invokeIPC('CONFIG_READ'),
        write: (config) => invokeIPC('CONFIG_WRITE', config),
    },

    // Main process log forwarding
    onMainLog: menuEventHandler(IPC_CHANNELS.MAIN_LOG),

    // Wavs folder change notification
    onWavsChange: menuEventHandler(IPC_CHANNELS.WAVS_ON_CHANGE),

    // OS shell operations
    shell: {
        openPath: (...args) => invokeIPC('SHELL_OPEN_PATH', ...args),
    },

    // Update operations
    update: {
        check: () => ipcRenderer.invoke(IPC_CHANNELS.UPDATE_CHECK),
        download: () => ipcRenderer.invoke(IPC_CHANNELS.UPDATE_DOWNLOAD),
        install: () => ipcRenderer.invoke(IPC_CHANNELS.UPDATE_INSTALL),
        onAvailable: menuEventHandler<[UpdateAvailableInfo]>(
            IPC_CHANNELS.UPDATE_AVAILABLE,
        ),
        onDownloaded: menuEventHandler(IPC_CHANNELS.UPDATE_DOWNLOADED),
        onDownloading: menuEventHandler(IPC_CHANNELS.UPDATE_DOWNLOADING),
        onError: menuEventHandler<[string]>(IPC_CHANNELS.UPDATE_ERROR),
    },
};

// Expose the API to the renderer process
contextBridge.exposeInMainWorld('electronAPI', electronAPI);

function menuEventHandler<T extends any[]>(
    channel: string,
): (callback: (...args: T) => void) => () => Electron.IpcRenderer {
    return (callback: (...args: T) => void) => {
        const subscription = (_event: any, ...args: T) => callback(...args);
        ipcRenderer.on(channel, subscription);
        return () => ipcRenderer.removeListener(channel, subscription);
    };
}
