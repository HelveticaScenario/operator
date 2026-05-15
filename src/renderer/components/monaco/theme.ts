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

    monaco.editor.defineTheme(monacoThemeId, {
        base: appTheme.type === 'light' ? 'vs' : 'vs-dark',
        colors: raw.colors,
        inherit: true,
        rules,
    });

    monaco.editor.setTheme(monacoThemeId);
}
