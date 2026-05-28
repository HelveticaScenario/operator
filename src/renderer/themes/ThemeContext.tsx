import React, { createContext, useContext, useEffect, useState } from 'react';
import type { AppTheme } from './types';
import { mapVSCodeTheme } from './types';
import { bundledThemes } from './bundled';
import electronAPI from '../electronAPI';
import type { PrettierConfig } from '../../shared/ipcTypes';

type CursorStyle =
    | 'line'
    | 'block'
    | 'underline'
    | 'line-thin'
    | 'block-outline'
    | 'underline-thin';

interface ThemeContextValue {
    theme: AppTheme;
    themes: AppTheme[];
    cursorStyle: CursorStyle;
    font: string;
    fontLigatures: boolean;
    fontSize: number;
    prettierConfig: PrettierConfig;
    xyScopeIntensity: number;
    xyScopePersistence: number;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

// Build theme lookup map
const themeMap = new Map<string, AppTheme>();
bundledThemes.forEach((t) => {
    const mapped = mapVSCodeTheme(t);
    themeMap.set(mapped.id, mapped);
});

function getThemeById(id: string): AppTheme {
    return themeMap.get(id) || mapVSCodeTheme(bundledThemes[0]);
}

// Apply theme colors to CSS custom properties
function applyThemeToCSS(theme: AppTheme) {
    const root = document.documentElement;
    const c = theme.colors;

    // Map our theme colors to CSS custom properties
    root.style.setProperty('--bg-primary', c.bgPrimary);
    root.style.setProperty('--bg-secondary', c.bgSecondary);
    root.style.setProperty('--bg-tertiary', c.bgTertiary);
    root.style.setProperty('--bg-hover', c.bgHover);
    root.style.setProperty('--bg-active', c.bgActive);

    root.style.setProperty('--border-subtle', c.borderSubtle);
    root.style.setProperty('--border-default', c.borderDefault);

    root.style.setProperty('--text-primary', c.textPrimary);
    root.style.setProperty('--text-secondary', c.textSecondary);
    root.style.setProperty('--text-muted', c.textMuted);
    root.style.setProperty('--text-bright', c.textBright);

    root.style.setProperty('--accent-primary', c.accentPrimary);
    root.style.setProperty('--accent-secondary', c.accentSecondary);
    root.style.setProperty('--accent-muted', c.accentMuted);

    root.style.setProperty('--color-success', c.colorSuccess);
    root.style.setProperty('--color-warning', c.colorWarning);
    root.style.setProperty('--color-error', c.colorError);
    root.style.setProperty('--color-info', c.colorInfo);

    // Update color-scheme for scrollbars etc
    root.style.colorScheme = theme.type;
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
    const [themes] = useState<AppTheme[]>(() => Array.from(themeMap.values()));
    const [theme, setTheme] = useState<AppTheme>(() =>
        getThemeById('modular-dark'),
    );
    const [cursorStyle, setCursorStyle] = useState<CursorStyle>('line');
    const [font, setFont] = useState('Fira Code');
    const [fontLigatures, setFontLigatures] = useState(true);
    const [fontSize, setFontSize] = useState(17);
    const [prettierConfig, setPrettierConfig] = useState<PrettierConfig>({});
    const [xyScopeIntensity, setXyScopeIntensity] = useState(0.6);
    const [xyScopePersistence, setXyScopePersistence] = useState(0.85);

    // Load initial config and set up watcher
    useEffect(() => {
        let unsubscribe: (() => void) | null = null;

        async function init() {
            // Read initial config
            const config = await electronAPI.config.read();
            if (config.theme) {
                setTheme(getThemeById(config.theme));
            }
            if (config.cursorStyle) {
                setCursorStyle(config.cursorStyle);
            }
            if (config.font) {
                setFont(config.font);
            }
            if (config.fontLigatures != null) {
                setFontLigatures(config.fontLigatures);
            }
            if (config.fontSize != null) {
                setFontSize(config.fontSize);
            }
            if (config.prettier) {
                setPrettierConfig(config.prettier);
            }
            if (config.xyScopeIntensity != null) {
                setXyScopeIntensity(config.xyScopeIntensity);
            }
            if (config.xyScopePersistence != null) {
                setXyScopePersistence(config.xyScopePersistence);
            }

            // Subscribe to config changes
            unsubscribe = electronAPI.config.onChange((newConfig) => {
                if (newConfig.theme) {
                    setTheme(getThemeById(newConfig.theme));
                }
                if (newConfig.cursorStyle) {
                    setCursorStyle(newConfig.cursorStyle);
                }
                if (newConfig.font) {
                    setFont(newConfig.font);
                }
                if (newConfig.fontLigatures != null) {
                    setFontLigatures(newConfig.fontLigatures);
                }
                if (newConfig.fontSize != null) {
                    setFontSize(newConfig.fontSize);
                }
                if (newConfig.prettier) {
                    setPrettierConfig(newConfig.prettier);
                }
                if (newConfig.xyScopeIntensity != null) {
                    setXyScopeIntensity(newConfig.xyScopeIntensity);
                }
                if (newConfig.xyScopePersistence != null) {
                    setXyScopePersistence(newConfig.xyScopePersistence);
                }
            });
        }

        void init();

        return () => {
            if (unsubscribe) {
                unsubscribe();
            }
        };
    }, []);

    // Apply theme to CSS whenever it changes
    useEffect(() => {
        applyThemeToCSS(theme);
    }, [theme]);

    return (
        <ThemeContext.Provider
            value={{
                cursorStyle,
                font,
                fontLigatures,
                fontSize,
                prettierConfig,
                theme,
                themes,
                xyScopeIntensity,
                xyScopePersistence,
            }}
        >
            {children}
        </ThemeContext.Provider>
    );
}

export function useTheme() {
    const ctx = useContext(ThemeContext);
    if (!ctx) {
        throw new Error('useTheme must be used within ThemeProvider');
    }
    return ctx;
}
