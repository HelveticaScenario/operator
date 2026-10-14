import type { AppConfig, MonospaceFont } from '../../shared/ipcTypes';
import { DEFAULT_PERFORMANCE_VIEW } from '../performance/settings';
import type { AppTheme } from '../themes/types';

interface PerformanceSettingsProps {
    config: AppConfig;
    themes: AppTheme[];
    /** The font options, grouped as the editor's font menu groups them. */
    fontGroups: { label: string; fonts: readonly string[] }[];
    onConfigChange: (partial: Partial<AppConfig>) => void;
}

function RangeSetting({
    label,
    min,
    max,
    step,
    value,
    format,
    onChange,
}: {
    label: string;
    min: number;
    max: number;
    step: number;
    value: number;
    format: (value: number) => string;
    onChange: (value: number) => void;
}) {
    return (
        <div className="settings-section">
            <h3>{label}</h3>
            <div className="settings-row">
                <input
                    type="range"
                    className="settings-range"
                    min={min}
                    max={max}
                    step={step}
                    value={value}
                    onChange={(e) => onChange(Number(e.target.value))}
                />
                <span className="settings-range-value">{format(value)}</span>
            </div>
        </div>
    );
}

function ToggleSetting({
    label,
    checked,
    onChange,
}: {
    label: string;
    checked: boolean;
    onChange: (checked: boolean) => void;
}) {
    return (
        <div className="settings-section">
            <h3>{label}</h3>
            <label className="settings-toggle-label">
                <input
                    type="checkbox"
                    className="settings-toggle"
                    checked={checked}
                    onChange={(e) => onChange(e.target.checked)}
                />
            </label>
        </div>
    );
}

/**
 * The performance window's settings: its shape and resolution, and how the
 * code looks there, apart from the editor's own look.
 */
export function PerformanceSettings({
    config,
    themes,
    fontGroups,
    onConfigChange,
}: PerformanceSettingsProps) {
    const d = DEFAULT_PERFORMANCE_VIEW;
    const fixed = (digits: number) => (value: number) => value.toFixed(digits);
    return (
        <>
            <div className="settings-section">
                <h3>Window Shape</h3>
                <select
                    className="device-select"
                    value={config.performanceAspect ?? 'free'}
                    onChange={(e) =>
                        onConfigChange({
                            performanceAspect: e.target.value as NonNullable<
                                AppConfig['performanceAspect']
                            >,
                        })
                    }
                >
                    <option value="free">Free</option>
                    <option value="16:9">16:9</option>
                    <option value="4:3">4:3</option>
                    <option value="1:1">1:1</option>
                    <option value="9:16">9:16</option>
                </select>
            </div>

            <div className="settings-section">
                <h3>Window Resolution</h3>
                <select
                    className="device-select"
                    value={String(config.performanceScale ?? 1)}
                    onChange={(e) =>
                        onConfigChange({
                            performanceScale: Number(e.target.value),
                        })
                    }
                >
                    <option value="1">100%</option>
                    <option value="0.75">75%</option>
                    <option value="0.5">50%</option>
                    <option value="0.25">25%</option>
                </select>
            </div>

            <ToggleSetting
                label="Show Code"
                checked={config.performanceCodeVisible ?? d.codeVisible}
                onChange={(performanceCodeVisible) =>
                    onConfigChange({ performanceCodeVisible })
                }
            />
            <RangeSetting
                label="Code Opacity"
                min={0}
                max={1}
                step={0.01}
                value={config.performanceCodeOpacity ?? d.codeOpacity}
                format={fixed(2)}
                onChange={(performanceCodeOpacity) =>
                    onConfigChange({ performanceCodeOpacity })
                }
            />
            <RangeSetting
                label="Code Backing"
                min={0}
                max={1}
                step={0.01}
                value={config.performanceCodeBacking ?? d.backing}
                format={fixed(2)}
                onChange={(performanceCodeBacking) =>
                    onConfigChange({ performanceCodeBacking })
                }
            />

            <div className="settings-section">
                <h3>Code Theme</h3>
                <select
                    className="device-select"
                    value={config.performanceTheme ?? ''}
                    onChange={(e) =>
                        onConfigChange({
                            performanceTheme: e.target.value || undefined,
                        })
                    }
                >
                    <option value="">Same as editor</option>
                    {themes.map((t) => (
                        <option key={t.id} value={t.id}>
                            {t.name}
                        </option>
                    ))}
                </select>
            </div>

            <div className="settings-section">
                <h3>Code Font</h3>
                <select
                    className="device-select"
                    value={config.performanceFont ?? d.font}
                    onChange={(e) =>
                        onConfigChange({
                            performanceFont: e.target.value as MonospaceFont,
                        })
                    }
                >
                    {fontGroups
                        .filter(({ fonts }) => fonts.length > 0)
                        .map(({ label, fonts }) => (
                            <optgroup key={label} label={label}>
                                {fonts.map((f) => (
                                    <option key={f} value={f}>
                                        {f}
                                    </option>
                                ))}
                            </optgroup>
                        ))}
                </select>
            </div>

            <RangeSetting
                label="Code Font Size"
                min={8}
                max={160}
                step={1}
                value={config.performanceFontSize ?? d.fontSize}
                format={(value) => `${value}px`}
                onChange={(performanceFontSize) =>
                    onConfigChange({ performanceFontSize })
                }
            />
            <RangeSetting
                label="Code Line Height"
                min={1}
                max={3}
                step={0.05}
                value={config.performanceLineHeight ?? d.lineHeight}
                format={fixed(2)}
                onChange={(performanceLineHeight) =>
                    onConfigChange({ performanceLineHeight })
                }
            />
            <ToggleSetting
                label="Code Ligatures"
                checked={config.performanceFontLigatures ?? d.ligatures}
                onChange={(performanceFontLigatures) =>
                    onConfigChange({ performanceFontLigatures })
                }
            />
            <ToggleSetting
                label="Code Line Numbers"
                checked={config.performanceLineNumbers ?? d.lineNumbers}
                onChange={(performanceLineNumbers) =>
                    onConfigChange({ performanceLineNumbers })
                }
            />
        </>
    );
}
