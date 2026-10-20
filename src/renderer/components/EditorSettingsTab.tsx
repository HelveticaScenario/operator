import React from 'react';
import type { AppConfig, MonospaceFont } from '../../shared/ipcTypes';
import type { AppTheme } from '../themes/types';
import { BUNDLED_FONTS } from '../../shared/fonts';
import { useInstalledSystemFonts } from './fonts';

type CursorStyle =
    | 'line'
    | 'block'
    | 'underline'
    | 'line-thin'
    | 'block-outline'
    | 'underline-thin';

const CURSOR_STYLES: CursorStyle[] = [
    'line',
    'block',
    'underline',
    'line-thin',
    'block-outline',
    'underline-thin',
];

interface EditorSettingsTabProps {
    config: AppConfig;
    themes: AppTheme[];
    onConfigChange: (partial: Partial<AppConfig>) => void;
}

export function EditorSettingsTab({
    config,
    themes,
    onConfigChange,
}: EditorSettingsTabProps) {
    const availableSystemFonts = useInstalledSystemFonts();

    return (
        <div className="settings-tab-content">
            {/* Theme */}
            <div className="settings-section">
                <h3>Color Theme</h3>
                <select
                    className="device-select"
                    value={config.theme || 'modular-dark'}
                    onChange={(e) => onConfigChange({ theme: e.target.value })}
                >
                    {themes.map((t) => (
                        <option key={t.id} value={t.id}>
                            {t.name}
                        </option>
                    ))}
                </select>
            </div>

            {/* Font */}
            <div className="settings-section">
                <h3>Font</h3>
                <select
                    className="device-select"
                    value={config.font || 'Fira Code'}
                    onChange={(e) =>
                        onConfigChange({
                            font: e.target.value as MonospaceFont,
                        })
                    }
                >
                    <optgroup label="Bundled">
                        {BUNDLED_FONTS.map((f) => (
                            <option key={f} value={f}>
                                {f}
                            </option>
                        ))}
                    </optgroup>
                    {availableSystemFonts.length > 0 && (
                        <optgroup label="System">
                            {availableSystemFonts.map((f) => (
                                <option key={f} value={f}>
                                    {f}
                                </option>
                            ))}
                        </optgroup>
                    )}
                </select>
            </div>

            {/* Font Size */}
            <div className="settings-section">
                <h3>Font Size</h3>
                <div className="settings-row">
                    <input
                        type="range"
                        className="settings-range"
                        min={8}
                        max={72}
                        step={1}
                        value={config.fontSize ?? 17}
                        onChange={(e) =>
                            onConfigChange({ fontSize: Number(e.target.value) })
                        }
                    />
                    <span className="settings-range-value">
                        {config.fontSize ?? 17}px
                    </span>
                </div>
            </div>

            {/* Font Ligatures */}
            <div className="settings-section">
                <h3>Font Ligatures</h3>
                <label className="settings-toggle-label">
                    <input
                        type="checkbox"
                        className="settings-toggle"
                        checked={config.fontLigatures ?? true}
                        onChange={(e) =>
                            onConfigChange({ fontLigatures: e.target.checked })
                        }
                    />
                </label>
            </div>

            {/* Cursor Style */}
            <div className="settings-section">
                <h3>Cursor Style</h3>
                <select
                    className="device-select"
                    value={config.cursorStyle || 'block'}
                    onChange={(e) =>
                        onConfigChange({
                            cursorStyle: e.target.value as CursorStyle,
                        })
                    }
                >
                    {CURSOR_STYLES.map((s) => (
                        <option key={s} value={s}>
                            {s}
                        </option>
                    ))}
                </select>
            </div>

            {/* Code Backdrop Opacity */}
            <div className="settings-section">
                <h3>Code Backing Over Video</h3>
                <div className="settings-row">
                    <input
                        type="range"
                        className="settings-range"
                        min={0}
                        max={1}
                        step={0.01}
                        value={config.codeBackdropOpacity ?? 0.6}
                        onChange={(e) =>
                            onConfigChange({
                                codeBackdropOpacity: Number(e.target.value),
                            })
                        }
                    />
                    <span className="settings-range-value">
                        {(config.codeBackdropOpacity ?? 0.6).toFixed(2)}
                    </span>
                </div>
            </div>
        </div>
    );
}
