import type { AppTheme } from '../../themes/types';
import type { Monaco } from '../../hooks/useCustomMonaco';

function withHexAlpha(hex: string, alpha: number): string {
    if (!hex.startsWith('#')) return hex;
    const body = hex.slice(1);
    const a = Math.round(Math.max(0, Math.min(1, alpha)) * 255)
        .toString(16)
        .padStart(2, '0');
    if (body.length === 8) return `#${body.slice(0, 6)}${a}`;
    if (body.length === 6) return `#${body}${a}`;
    if (body.length === 3) {
        return `#${body[0]}${body[0]}${body[1]}${body[1]}${body[2]}${body[2]}${a}`;
    }
    return hex;
}

export function applyMonacoTheme(
    monaco: Monaco,
    appTheme: AppTheme,
    monacoThemeId: string,
) {
    const { raw } = appTheme;

    const rules = raw.tokenColors
        .map((tc) => {
            const scopes = Array.isArray(tc.scope)
                ? tc.scope
                : [tc.scope || ''];
            return scopes.map((scope) => ({
                background: tc.settings.background?.replace('#', ''),
                fontStyle: tc.settings.fontStyle,
                foreground: tc.settings.foreground?.replace('#', ''),
                token: scope.replace(/\./g, ' ').trim() || '',
            }));
        })
        .flat();

    // Force editor chrome transparent so the $scopeXY background canvas
    // shows through. Keep all other token colours from the theme intact.
    const TRANSPARENT = '#00000000';
    const lineHighlightAlpha = withHexAlpha(
        raw.colors?.['editor.lineHighlightBackground'] ?? '#000000',
        0.4,
    );
    monaco.editor.defineTheme(monacoThemeId, {
        base: appTheme.type === 'light' ? 'vs' : 'vs-dark',
        colors: {
            ...raw.colors,
            'editor.background': TRANSPARENT,
            'editorGutter.background': TRANSPARENT,
            'minimap.background': TRANSPARENT,
            'editorStickyScroll.background': TRANSPARENT,
            'editorStickyScrollHover.background': TRANSPARENT,
            'editor.lineHighlightBackground': lineHighlightAlpha,
            // Match border to fill so the 1px outline Monaco draws around
            // the current line blends in — otherwise the semi-transparent
            // fill makes it read as a hard rectangle.
            'editor.lineHighlightBorder': lineHighlightAlpha,
        },
        inherit: true,
        rules,
    });

    monaco.editor.setTheme(monacoThemeId);
}
