import type { editor } from 'monaco-editor';
import { useEffect, useMemo } from 'react';
import { useCustomMonaco } from '../hooks/useCustomMonaco';
import type { AppTheme } from '../themes/types';
import { performanceOutput } from '../video/PerformanceOutput';
import { PerformanceCode } from './PerformanceCode';
import type { PerformanceViewSettings } from './settings';

/**
 * Lays the code of `codeEditor` over the performance window's picture, drawn
 * with `settings` in the colors of `theme`.
 */
export function usePerformanceCode(
    codeEditor: editor.IStandaloneCodeEditor | null,
    settings: PerformanceViewSettings,
    theme: AppTheme,
): void {
    const monaco = useCustomMonaco();
    const code = useMemo(
        () => (monaco === null ? null : new PerformanceCode(monaco)),
        [monaco],
    );

    useEffect(() => {
        if (code === null) return;
        performanceOutput.setCode(code);
        return () => {
            performanceOutput.setCode(null);
            code.dispose();
        };
    }, [code]);

    useEffect(() => {
        code?.attach(codeEditor);
    }, [code, codeEditor]);

    useEffect(() => {
        code?.setLook(settings, theme);
    }, [code, settings, theme]);

    useEffect(() => {
        performanceOutput.setCodeOpacity(
            settings.codeVisible ? settings.codeOpacity : 0,
        );
    }, [settings.codeVisible, settings.codeOpacity]);
}
