import { useEffect, useState } from 'react';
import { useEditableConfig } from '../hooks/useEditableConfig';
import { useTheme } from '../themes/ThemeContext';
import { performanceOutput } from '../video/PerformanceOutput';
import { BUNDLED_FONTS } from '../../shared/fonts';
import { useInstalledSystemFonts } from './fonts';
import { PerformanceSettings } from './PerformanceSettings';
import './Settings.css';
import './PerformancePanel.css';

/**
 * The performance window's settings in the sidebar, beside the editor, so they
 * can be changed while watching the preview of that window.
 */
export function PerformancePanel() {
    const [config, change] = useEditableConfig(true);
    const { themes } = useTheme();
    const systemFonts = useInstalledSystemFonts();
    const [open, setOpen] = useState(performanceOutput.isOpen);

    useEffect(
        () =>
            performanceOutput.subscribe(() => {
                setOpen(performanceOutput.isOpen);
            }),
        [],
    );

    return (
        <div className="performance-panel">
            <button
                className="performance-panel-toggle"
                onClick={() => performanceOutput.toggle()}
            >
                {open ? 'Close Performance Window' : 'Open Performance Window'}
            </button>
            <PerformanceSettings
                config={config}
                themes={themes}
                fontGroups={[
                    { fonts: BUNDLED_FONTS, label: 'Bundled' },
                    { fonts: systemFonts, label: 'System' },
                ]}
                onConfigChange={change}
            />
        </div>
    );
}
