import { useCallback, useEffect, useState } from 'react';
import type { AppConfig } from '../../shared/ipcTypes';
import electronAPI from '../electronAPI';

/**
 * The config file while `active`, kept current with outside edits, and a
 * function that merges a change into it and saves it at once.
 */
export function useEditableConfig(
    active: boolean,
): [AppConfig, (partial: Partial<AppConfig>) => void] {
    const [config, setConfig] = useState<AppConfig>({});

    useEffect(() => {
        if (!active) return;
        electronAPI.config.read().then(setConfig).catch(console.error);
        return electronAPI.config.onChange(setConfig);
    }, [active]);

    const change = useCallback((partial: Partial<AppConfig>) => {
        setConfig((prev) => {
            const updated = { ...prev, ...partial };
            electronAPI.config.write(updated).catch(console.error);
            return updated;
        });
    }, []);

    return [config, change];
}
