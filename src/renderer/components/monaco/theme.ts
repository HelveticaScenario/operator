import type { AppTheme } from '../../themes/types';
import type { Monaco } from '../../hooks/useCustomMonaco';

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

    // Force editor chrome transparent so the $scopeXY woscope canvas shows
    // through. Keep all other token colours from the theme intact.
    const TRANSPARENT = '#00000000';
    monaco.editor.defineTheme(monacoThemeId, {
        base: appTheme.type === 'light' ? 'vs' : 'vs-dark',
        colors: {
            ...raw.colors,
            'editor.background': TRANSPARENT,
            'editorGutter.background': TRANSPARENT,
            'minimap.background': TRANSPARENT,
            'editorStickyScroll.background': TRANSPARENT,
            'editorStickyScrollHover.background': TRANSPARENT,
        },
        inherit: true,
        rules,
    });

    monaco.editor.setTheme(monacoThemeId);
}
