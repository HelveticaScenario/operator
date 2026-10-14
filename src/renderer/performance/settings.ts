import type { AppConfig, MonospaceFont } from '../../shared/ipcTypes';

/** How the performance window shows the code, set apart from the editor's own look. */
export interface PerformanceViewSettings {
    /** Whether the code shows at all; toggling it fades the code in or out. */
    codeVisible: boolean;
    /** Opacity of the code while it shows, 0 to 1. */
    codeOpacity: number;
    /** Opacity of the backing behind each line of code and of the cursor line's highlight, 0 to 1. */
    backing: number;
    font: MonospaceFont;
    /** Font size in points of the performance window. */
    fontSize: number;
    /** Line height as a multiple of the font size. */
    lineHeight: number;
    ligatures: boolean;
    lineNumbers: boolean;
    /** Theme id for the code's colors; null follows the editor's theme. */
    themeId: string | null;
    /** How quickly the view scrolls to where it should be, 0 to 1; see {@link scrollEaseMs}. */
    scrollSpeed: number;
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
    scrollSpeed: 0.5,
    themeId: null,
};

/** Scroll easing time constants, in milliseconds, at the slowest and fastest speeds short of instant. */
const SLOWEST_SCROLL_MS = 1000;
const FASTEST_SCROLL_MS = 10;

/**
 * Time constant, in milliseconds, of the view easing toward where it should
 * scroll at `scrollSpeed`: from 1 s at 0 to 10 ms just below 1, evenly on a
 * log scale, and 0 (a jump) at 1.
 */
export function scrollEaseMs(scrollSpeed: number): number {
    return scrollSpeed >= 1
        ? 0
        : SLOWEST_SCROLL_MS *
              (FASTEST_SCROLL_MS / SLOWEST_SCROLL_MS) ** scrollSpeed;
}

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
        scrollSpeed: config.performanceScrollSpeed ?? d.scrollSpeed,
        themeId: config.performanceTheme ?? d.themeId,
    };
}
