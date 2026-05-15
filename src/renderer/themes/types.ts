// VS Code theme structure types

export interface VSCodeTokenColor {
    name?: string;
    scope?: string | string[];
    settings: {
        foreground?: string;
        background?: string;
        fontStyle?: string;
    };
}

export interface VSCodeTheme {
    name: string;
    type: 'dark' | 'light' | 'hc';
    colors: Record<string, string>;
    tokenColors: VSCodeTokenColor[];
}

// Simplified theme for our app
export interface AppTheme {
    id: string;
    name: string;
    type: 'dark' | 'light';
    colors: {
        // Backgrounds
        bgPrimary: string;
        bgSecondary: string;
        bgTertiary: string;
        bgHover: string;
        bgActive: string;

        // Borders
        borderSubtle: string;
        borderDefault: string;

        // Text
        textPrimary: string;
        textSecondary: string;
        textMuted: string;
        textBright: string;

        // Accent
        accentPrimary: string;
        accentSecondary: string;
        accentMuted: string;

        // Semantic
        colorSuccess: string;
        colorWarning: string;
        colorError: string;
        colorInfo: string;

        // Editor specific
        editorBackground: string;
        editorForeground: string;
        editorLineHighlight: string;
        editorSelection: string;
        editorCursor: string;
        lineNumberForeground: string;
        lineNumberActiveForeground: string;
    };
    // Raw VS Code theme for Monaco
    raw: VSCodeTheme;
}

// Map VS Code workbench colors to our app theme colors
export function mapVSCodeTheme(vscodeTheme: VSCodeTheme): AppTheme {
    const c = vscodeTheme.colors;
    const type = vscodeTheme.type === 'light' ? 'light' : 'dark';

    // Helper to get color with fallback
    const get = (key: string, fallback: string): string => c[key] || fallback;

    // Default fallbacks based on theme type
    const defaults =
        type === 'dark'
            ? {
                  accent: '#007acc',
                  bgPrimary: '#1e1e1e',
                  bgSecondary: '#252526',
                  bgTertiary: '#2d2d2d',
                  textMuted: '#555555',
                  textPrimary: '#cccccc',
                  textSecondary: '#888888',
              }
            : {
                  accent: '#007acc',
                  bgPrimary: '#ffffff',
                  bgSecondary: '#f3f3f3',
                  bgTertiary: '#e8e8e8',
                  textMuted: '#999999',
                  textPrimary: '#333333',
                  textSecondary: '#666666',
              };

    return {
        colors: {
            // Backgrounds - map from VS Code workbench colors
            bgPrimary: get('editor.background', defaults.bgPrimary),
            bgSecondary: get(
                'sideBar.background',
                get('activityBar.background', defaults.bgSecondary),
            ),
            bgTertiary: get(
                'sideBarSectionHeader.background',
                get('tab.activeBackground', defaults.bgTertiary),
            ),
            bgHover: get(
                'list.hoverBackground',
                adjustAlpha(defaults.textPrimary, 0.1),
            ),
            bgActive: get(
                'list.activeSelectionBackground',
                get('editor.selectionBackground', defaults.accent),
            ),

            // Borders
            borderSubtle: get(
                'editorGroup.border',
                get('sideBar.border', adjustAlpha(defaults.textMuted, 0.3)),
            ),
            borderDefault: get(
                'input.border',
                get('dropdown.border', adjustAlpha(defaults.textMuted, 0.5)),
            ),

            // Text
            textPrimary: get(
                'editor.foreground',
                get('foreground', defaults.textPrimary),
            ),
            textSecondary: get(
                'descriptionForeground',
                get('sideBar.foreground', defaults.textSecondary),
            ),
            textMuted: get(
                'disabledForeground',
                get('editorLineNumber.foreground', defaults.textMuted),
            ),
            textBright: get('editor.foreground', defaults.textPrimary),

            // Accent
            accentPrimary: get(
                'focusBorder',
                get(
                    'button.background',
                    get('activityBarBadge.background', defaults.accent),
                ),
            ),
            accentSecondary: get(
                'textLink.foreground',
                get('activityBarBadge.background', defaults.accent),
            ),
            accentMuted: adjustAlpha(get('focusBorder', defaults.accent), 0.4),

            // Semantic
            colorSuccess: get(
                'terminal.ansiGreen',
                get('gitDecoration.addedResourceForeground', '#3fb27f'),
            ),
            colorWarning: get(
                'terminal.ansiYellow',
                get('editorWarning.foreground', '#d4a855'),
            ),
            colorError: get(
                'terminal.ansiRed',
                get('editorError.foreground', '#e05561'),
            ),
            colorInfo: get(
                'terminal.ansiBlue',
                get('editorInfo.foreground', '#61afef'),
            ),

            // Editor specific
            editorBackground: get('editor.background', defaults.bgPrimary),
            editorForeground: get('editor.foreground', defaults.textPrimary),
            editorLineHighlight: get(
                'editor.lineHighlightBackground',
                adjustAlpha(defaults.textPrimary, 0.05),
            ),
            editorSelection: get('editor.selectionBackground', defaults.accent),
            editorCursor: get('editorCursor.foreground', defaults.accent),
            lineNumberForeground: get(
                'editorLineNumber.foreground',
                defaults.textMuted,
            ),
            lineNumberActiveForeground: get(
                'editorLineNumber.activeForeground',
                defaults.textSecondary,
            ),
        },
        id: vscodeTheme.name.toLowerCase().replace(/\s+/g, '-'),
        name: vscodeTheme.name,
        raw: vscodeTheme,
        type,
    };
}

// Helper to adjust alpha of a hex color
function adjustAlpha(hex: string, alpha: number): string {
    // Handle already-rgba colors
    if (hex.startsWith('rgba')) {
        return hex;
    }
    if (hex.startsWith('rgb')) {
        const match = hex.match(/rgb\((\d+),\s*(\d+),\s*(\d+)\)/);
        if (match) {
            return `rgba(${match[1]}, ${match[2]}, ${match[3]}, ${alpha})`;
        }
    }

    // Handle hex colors
    const cleanHex = hex.replace('#', '');
    let r: number, g: number, b: number;

    if (cleanHex.length === 3) {
        r = parseInt(cleanHex[0] + cleanHex[0], 16);
        g = parseInt(cleanHex[1] + cleanHex[1], 16);
        b = parseInt(cleanHex[2] + cleanHex[2], 16);
    } else if (cleanHex.length === 6) {
        r = parseInt(cleanHex.slice(0, 2), 16);
        g = parseInt(cleanHex.slice(2, 4), 16);
        b = parseInt(cleanHex.slice(4, 6), 16);
    } else if (cleanHex.length === 8) {
        // Already has alpha
        r = parseInt(cleanHex.slice(0, 2), 16);
        g = parseInt(cleanHex.slice(2, 4), 16);
        b = parseInt(cleanHex.slice(4, 6), 16);
    } else {
        return hex;
    }

    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
