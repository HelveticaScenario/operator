import type { editor } from 'monaco-editor';
import { useCallback, useEffect, useMemo } from 'react';
import { useCustomMonaco } from '../hooks/useCustomMonaco';
import type { AppTheme } from '../themes/types';
import { performanceOutput } from '../video/PerformanceOutput';
import { PerformanceCode, type PlacedControl } from './PerformanceCode';
import type { PerformanceViewSettings } from './settings';

/**
 * Lays the code of `codeEditor` over the performance window's picture, drawn
 * with `settings` in the colors of `theme`, with a widget for each of
 * `controls`. Returns a function that scrolls the performance view to an
 * offset in the editor's text (see {@link PerformanceCode.reveal}).
 */
export function usePerformanceCode(
    codeEditor: editor.IStandaloneCodeEditor | null,
    settings: PerformanceViewSettings,
    theme: AppTheme,
    controls: readonly PlacedControl[],
): (offset: number) => void {
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
        code?.setControls(controls);
    }, [code, controls]);

    useEffect(() => {
        code?.setLook(settings, theme);
    }, [code, settings, theme]);

    useEffect(() => {
        performanceOutput.setCodeOpacity(
            settings.codeVisible ? settings.codeOpacity : 0,
        );
    }, [settings.codeVisible, settings.codeOpacity]);

    return useCallback((offset: number) => code?.reveal(offset), [code]);
}
