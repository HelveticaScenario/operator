import React, {
    forwardRef,
    useCallback,
    useEffect,
    useImperativeHandle,
    useMemo,
    useState,
} from 'react';
import electronAPI from '../electronAPI';
import type {
    AudioDeviceInfo,
    CurrentAudioState,
    DeviceCacheSnapshot,
} from '../../shared/ipcTypes';

export interface AudioSettingsHandle {
    apply: () => Promise<void>;
    isDirty: () => boolean;
}

interface AudioSettingsTabProps {
    isActive: boolean;
}

/**
 * Compute intersection of sample rates supported by given devices.
 * If only output device is provided, returns its supported rates.
 */
function computeCommonSampleRates(
    outputDevice: AudioDeviceInfo | null,
    inputDevice: AudioDeviceInfo | null,
): number[] {
    if (!outputDevice) {
        return [];
    }

    const outputRates = outputDevice.supportedSampleRates || [];
    if (!inputDevice) {
        return [...outputRates].sort((a, b) => a - b);
    }

    const inputRates = new Set(inputDevice.supportedSampleRates || []);
    return outputRates.filter((r) => inputRates.has(r)).sort((a, b) => a - b);
}

/**
 * Compute buffer sizes available for given devices.
 * Returns power-of-2 values (64, 128, 256, 512, 1024, 2048) within the supported range.
 */
function computeBufferSizes(
    outputDevice: AudioDeviceInfo | null,
    inputDevice: AudioDeviceInfo | null,
): number[] {
    const powerOf2Sizes = [64, 128, 256, 512, 1024, 2048, 4096];

    if (!outputDevice) {
        return [];
    }

    // Find the common range
    let minSize = outputDevice.bufferSizeRange?.min ?? 64;
    let maxSize = outputDevice.bufferSizeRange?.max ?? 4096;

    if (inputDevice?.bufferSizeRange) {
        minSize = Math.max(minSize, inputDevice.bufferSizeRange.min);
        maxSize = Math.min(maxSize, inputDevice.bufferSizeRange.max);
    }

    return powerOf2Sizes.filter((size) => size >= minSize && size <= maxSize);
}

export const AudioSettingsTab = forwardRef<
    AudioSettingsHandle,
    AudioSettingsTabProps
>(function AudioSettingsTab({ isActive }, ref) {
    // Device cache from Rust
    const [deviceCache, setDeviceCache] = useState<DeviceCacheSnapshot | null>(
        null,
    );
    const [currentState, setCurrentState] = useState<CurrentAudioState | null>(
        null,
    );

    // Current selections (pending changes, not yet applied)
    const [selectedHostId, setSelectedHostId] = useState<string | null>(null);
    const [selectedOutputDeviceId, setSelectedOutputDeviceId] = useState<
        string | null
    >(null);
    const [selectedInputDeviceId, setSelectedInputDeviceId] = useState<
        string | null
    >(null);
    const [selectedSampleRate, setSelectedSampleRate] = useState<number | null>(
        null,
    );
    const [selectedBufferSize, setSelectedBufferSize] = useState<number | null>(
        null,
    );

    // UI state
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [warning, setWarning] = useState<string | null>(null);
    const [applying, setApplying] = useState(false);

    // Filter devices by selected host
    const outputDevicesForHost = useMemo(() => {
        if (!deviceCache || !selectedHostId) {
            return [];
        }
        const hostData = deviceCache.hosts.find(
            (h) => h.hostId === selectedHostId,
        );
        return hostData?.outputDevices ?? [];
    }, [deviceCache, selectedHostId]);

    const inputDevicesForHost = useMemo(() => {
        if (!deviceCache || !selectedHostId) {
            return [];
        }
        const hostData = deviceCache.hosts.find(
            (h) => h.hostId === selectedHostId,
        );
        return hostData?.inputDevices ?? [];
    }, [deviceCache, selectedHostId]);

    // Get selected device objects
    const selectedOutputDevice = useMemo(
        () =>
            outputDevicesForHost.find((d) => d.id === selectedOutputDeviceId) ??
            null,
        [outputDevicesForHost, selectedOutputDeviceId],
    );

    const selectedInputDevice = useMemo(
        () =>
            inputDevicesForHost.find((d) => d.id === selectedInputDeviceId) ??
            null,
        [inputDevicesForHost, selectedInputDeviceId],
    );

    // Compute available sample rates and buffer sizes (JS-side logic)
    const availableSampleRates = useMemo(
        () =>
            computeCommonSampleRates(selectedOutputDevice, selectedInputDevice),
        [selectedOutputDevice, selectedInputDevice],
    );

    const availableBufferSizes = useMemo(
        () => computeBufferSizes(selectedOutputDevice, selectedInputDevice),
        [selectedOutputDevice, selectedInputDevice],
    );

    // Load data from device cache
    const loadData = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);

            const [cache, state] = await Promise.all([
                electronAPI.audio.getDeviceCache(),
                electronAPI.audio.getCurrentState(),
            ]);

            setDeviceCache(cache);
            setCurrentState(state);

            // The engine keeps reporting a device fallback in its state until
            // the streams are recreated, so surface it even when this panel
            // opens long after the fallback happened.
            setWarning(state.fallbackWarning ?? null);

            // Initialize selections from current state
            setSelectedHostId(state.hostId);
            setSelectedOutputDeviceId(state.outputDeviceId ?? null);
            setSelectedInputDeviceId(state.inputDeviceId ?? null);
            setSelectedSampleRate(state.sampleRate);
            setSelectedBufferSize(state.bufferSize ?? null);
        } catch (err) {
            setError(
                err instanceof Error ? err.message : 'Failed to load settings',
            );
        } finally {
            setLoading(false);
        }
    }, []);

    // When host changes, reset device selections to defaults for that host
    useEffect(() => {
        if (!deviceCache || !selectedHostId) {
            return;
        }

        const hostData = deviceCache.hosts.find(
            (h) => h.hostId === selectedHostId,
        );
        if (!hostData) {
            return;
        }

        // If current output device isn't in this host, select default
        const outputInHost = hostData.outputDevices.find(
            (d) => d.id === selectedOutputDeviceId,
        );
        if (!outputInHost) {
            const defaultOutput =
                hostData.outputDevices.find((d) => d.isDefault) ??
                hostData.outputDevices[0];
            setSelectedOutputDeviceId(defaultOutput?.id ?? null);
        }

        // If current input device isn't in this host, select none or default
        const inputInHost = hostData.inputDevices.find(
            (d) => d.id === selectedInputDeviceId,
        );
        if (!inputInHost && selectedInputDeviceId !== null) {
            setSelectedInputDeviceId(null);
        }
    }, [
        deviceCache,
        selectedHostId,
        selectedInputDeviceId,
        selectedOutputDeviceId,
    ]);

    // Update sample rate and buffer size when device selection changes
    useEffect(() => {
        // Auto-select a sensible sample rate if the current one is not available.
        // Prefer the output device's native/default rate (from cpal) so we
        // Match what the OS considers optimal for this device.
        if (
            selectedSampleRate === null ||
            !availableSampleRates.includes(selectedSampleRate)
        ) {
            const deviceDefault = selectedOutputDevice?.sampleRate ?? null;
            const preferred =
                deviceDefault !== null &&
                availableSampleRates.includes(deviceDefault)
                    ? deviceDefault
                    : availableSampleRates.length > 0
                      ? availableSampleRates[availableSampleRates.length - 1]
                      : null;
            setSelectedSampleRate(preferred);
        }

        // Auto-select lowest buffer size if not in available list
        if (
            selectedBufferSize === null ||
            !availableBufferSizes.includes(selectedBufferSize)
        ) {
            const lowest =
                availableBufferSizes.length > 0
                    ? Math.min(...availableBufferSizes)
                    : null;
            setSelectedBufferSize(lowest);
        }
    }, [
        availableSampleRates,
        availableBufferSizes,
        selectedSampleRate,
        selectedBufferSize,
        selectedOutputDevice,
    ]);

    // Refresh devices from hardware
    const refreshDevices = useCallback(async () => {
        try {
            setLoading(true);
            await electronAPI.audio.refreshDeviceCache();
            await loadData();
        } catch (err) {
            setError(
                err instanceof Error
                    ? err.message
                    : 'Failed to refresh devices',
            );
            setLoading(false);
        }
    }, [loadData]);

    // Load on active
    useEffect(() => {
        if (isActive) {
            void loadData();
        }
    }, [isActive, loadData]);

    // Listen for fallback warnings
    useEffect(() => {
        const unsubscribe = electronAPI.audio.onFallbackWarning(
            (msg: string) => {
                setWarning(msg);
            },
        );
        return unsubscribe;
    }, []);

    // Apply configuration using recreateStreams
    const applyConfig = useCallback(async () => {
        if (!selectedOutputDeviceId) {
            setError('Please select an output device');
            return;
        }

        if (!selectedSampleRate) {
            setError('Please select a sample rate');
            return;
        }

        try {
            setApplying(true);
            setError(null);
            setWarning(null);

            await electronAPI.audio.recreateStreams(
                selectedOutputDeviceId,
                selectedSampleRate,
                selectedBufferSize ?? undefined,
                selectedInputDeviceId ?? undefined,
            );

            // Reload state to get actual applied values
            const newState = await electronAPI.audio.getCurrentState();
            setCurrentState(newState);

            // Update selections to match what was actually applied
            setSelectedHostId(newState.hostId);
            setSelectedOutputDeviceId(newState.outputDeviceId ?? null);
            setSelectedInputDeviceId(newState.inputDeviceId ?? null);
            setSelectedSampleRate(newState.sampleRate);
            setSelectedBufferSize(newState.bufferSize ?? null);
        } catch (err) {
            setError(
                err instanceof Error
                    ? err.message
                    : 'Failed to apply configuration',
            );
        } finally {
            setApplying(false);
        }
    }, [
        selectedOutputDeviceId,
        selectedSampleRate,
        selectedBufferSize,
        selectedInputDeviceId,
    ]);

    // Check if selections differ from current running state
    const checkIsDirty = useCallback(() => {
        if (!currentState) {
            return false;
        }
        return (
            selectedHostId !== currentState.hostId ||
            selectedOutputDeviceId !== (currentState.outputDeviceId ?? null) ||
            selectedInputDeviceId !== (currentState.inputDeviceId ?? null) ||
            selectedSampleRate !== currentState.sampleRate ||
            selectedBufferSize !== (currentState.bufferSize ?? null)
        );
    }, [
        currentState,
        selectedHostId,
        selectedOutputDeviceId,
        selectedInputDeviceId,
        selectedSampleRate,
        selectedBufferSize,
    ]);

    // Expose apply and isDirty to parent via ref
    useImperativeHandle(
        ref,
        () => ({
            apply: applyConfig,
            isDirty: checkIsDirty,
        }),
        [applyConfig, checkIsDirty],
    );

    // Only show hosts that have at least one available device
    const hosts =
        deviceCache?.hosts
            .filter(
                (h) => h.outputDevices.length > 0 || h.inputDevices.length > 0,
            )
            .map((h) => ({ id: h.hostId, name: h.hostName })) ?? [];

    return (
        <div className="settings-tab-content">
            {error && <div className="settings-tab-error">{error}</div>}

            {warning && <div className="settings-tab-warning">{warning}</div>}

            {loading ? (
                <div className="settings-tab-loading">Loading devices...</div>
            ) : (
                <>
                    {/* Audio Host */}
                    <div className="settings-section">
                        <h3>Audio Host</h3>
                        <select
                            className="device-select"
                            value={selectedHostId || ''}
                            onChange={(e) =>
                                setSelectedHostId(e.target.value || null)
                            }
                        >
                            {hosts.map((h) => (
                                <option key={h.id} value={h.id}>
                                    {h.name}
                                </option>
                            ))}
                        </select>
                    </div>

                    {/* Output Device */}
                    <div className="settings-section">
                        <h3>Output Device</h3>
                        <select
                            className="device-select"
                            value={selectedOutputDeviceId || ''}
                            onChange={(e) =>
                                setSelectedOutputDeviceId(
                                    e.target.value || null,
                                )
                            }
                        >
                            <option value="">-- Select Output Device --</option>
                            {outputDevicesForHost.map((d) => (
                                <option key={d.id} value={d.id}>
                                    {d.name} ({d.outputChannels} ch)
                                    {d.isDefault ? ' (Default)' : ''}
                                </option>
                            ))}
                        </select>
                        {selectedOutputDevice && (
                            <div className="device-info">
                                {selectedOutputDevice.sampleRate} Hz,{' '}
                                {selectedOutputDevice.outputChannels} channels
                            </div>
                        )}
                    </div>

                    {/* Input Device */}
                    <div className="settings-section">
                        <h3>Input Device</h3>
                        <select
                            className="device-select"
                            value={selectedInputDeviceId || ''}
                            onChange={(e) =>
                                setSelectedInputDeviceId(e.target.value || null)
                            }
                        >
                            <option value="">None (No Input)</option>
                            {inputDevicesForHost.map((d) => (
                                <option key={d.id} value={d.id}>
                                    {d.name} ({d.inputChannels} ch)
                                    {d.isDefault ? ' (Default)' : ''}
                                </option>
                            ))}
                        </select>
                        {selectedInputDevice && (
                            <div className="device-info">
                                {selectedInputDevice.sampleRate} Hz,{' '}
                                {selectedInputDevice.inputChannels} channels
                            </div>
                        )}
                    </div>

                    {/* Sample Rate */}
                    <div className="settings-section">
                        <h3>Sample Rate</h3>
                        <select
                            className="device-select"
                            value={selectedSampleRate || ''}
                            onChange={(e) =>
                                setSelectedSampleRate(
                                    Number(e.target.value) || null,
                                )
                            }
                            disabled={availableSampleRates.length === 0}
                        >
                            {availableSampleRates.length === 0 ? (
                                <option value="">Select devices first</option>
                            ) : (
                                availableSampleRates.map((rate) => (
                                    <option key={rate} value={rate}>
                                        {rate} Hz
                                    </option>
                                ))
                            )}
                        </select>
                    </div>

                    {/* Buffer Size */}
                    <div className="settings-section">
                        <h3>Buffer Size</h3>
                        <select
                            className="device-select"
                            value={selectedBufferSize || ''}
                            onChange={(e) =>
                                setSelectedBufferSize(
                                    Number(e.target.value) || null,
                                )
                            }
                            disabled={availableBufferSizes.length === 0}
                        >
                            {availableBufferSizes.length === 0 ? (
                                <option value="">Select devices first</option>
                            ) : (
                                availableBufferSizes.map((size) => (
                                    <option key={size} value={size}>
                                        {size} samples (~
                                        {(
                                            (size /
                                                (selectedSampleRate || 48000)) *
                                            1000
                                        ).toFixed(1)}{' '}
                                        ms)
                                    </option>
                                ))
                            )}
                        </select>
                    </div>

                    {/* Refresh Devices */}
                    <div className="settings-section">
                        <button
                            className="btn btn-secondary"
                            onClick={refreshDevices}
                            disabled={applying}
                        >
                            ↻ Refresh Devices
                        </button>
                    </div>
                </>
            )}
        </div>
    );
});
