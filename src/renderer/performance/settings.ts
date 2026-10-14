import type { AppConfig } from '../../shared/ipcTypes';

/** How the performance window shows the code, set apart from the editor's own look. */
export interface PerformanceViewSettings {
    /** Whether the code shows at all; toggling it fades the code in or out. */
    codeVisible: boolean;
    /** Opacity of the code while it shows, 0 to 1. */
    codeOpacity: number;
    /** Opacity of the backing behind each line of code and of the cursor line's highlight, 0 to 1. */
    backing: number;
    font: string;
    /** Font size in points of the performance window. */
    fontSize: number;
    /** Line height as a multiple of the font size. */
    lineHeight: number;
    ligatures: boolean;
    lineNumbers: boolean;
    /** Theme id for the code's colors; null follows the editor's theme. */
    themeId: string | null;
}

export const DEFAULT_PERFORMANCE_VIEW: PerformanceViewSettings = {
    backing: 0.6,
    codeOpacity: 1,
    codeVisible: true,
    font: 'Fira Code',
    fontSize: 28,
    ligatures: true,
    lineHeight: 1.4,
    lineNumbers: false,
    themeId: null,
};

/** The performance view settings in `config`, with defaults for the rest. */
export function performanceViewSettings(
    config: AppConfig,
): PerformanceViewSettings {
    const d = DEFAULT_PERFORMANCE_VIEW;
    return {
        backing: config.performanceCodeBacking ?? d.backing,
        codeOpacity: config.performanceCodeOpacity ?? d.codeOpacity,
        codeVisible: config.performanceCodeVisible ?? d.codeVisible,
        font: config.performanceFont ?? d.font,
        fontSize: config.performanceFontSize ?? d.fontSize,
        ligatures: config.performanceFontLigatures ?? d.ligatures,
        lineHeight: config.performanceLineHeight ?? d.lineHeight,
        lineNumbers: config.performanceLineNumbers ?? d.lineNumbers,
        themeId: config.performanceTheme ?? d.themeId,
    };
}
