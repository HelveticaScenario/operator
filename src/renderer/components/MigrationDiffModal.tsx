import { DiffEditor } from '@monaco-editor/react';
import { useCallback, useEffect, useRef } from 'react';

import { useTheme } from '../themes/ThemeContext';
import { applyMonacoTheme } from './monaco/theme';
import './MigrationDiffModal.css';

export interface MigrationModalSummary {
    callsChanged: number;
    assignmentsChanged: number;
    commentsChanged: number;
    skippedVariables: string[];
    error?: string;
}

interface Props {
    isOpen: boolean;
    original: string;
    migrated: string;
    summary: MigrationModalSummary;
    /** Heading shown in the modal and as the diff's purpose. */
    title?: string;
    /** Label prefixing the list of `skippedVariables` (calls/variables that
     *  could not be rewritten automatically). */
    skippedLabel?: string;
    onApply: () => void;
    onCancel: () => void;
}

export function MigrationDiffModal({
    isOpen,
    original,
    migrated,
    summary,
    title = 'Migrate $cycle / $iCycle to $p / $p.s',
    skippedLabel = 'Skipped variables (non-string or mixed assignments):',
    onApply,
    onCancel,
}: Props) {
    const panelRef = useRef<HTMLDivElement>(null);
    const { theme: appTheme, font, fontLigatures, fontSize } = useTheme();
    const monacoThemeId = `theme-${appTheme.id}`;

    const totalChanges =
        summary.callsChanged +
        summary.assignmentsChanged +
        summary.commentsChanged;
    const noChanges = totalChanges === 0;
    const canApply = !noChanges && !summary.error;

    const handleKey = useCallback(
        (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                onCancel();
            } else if (e.key === 'Enter' && canApply) {
                onApply();
            }
        },
        [canApply, onApply, onCancel],
    );

    useEffect(() => {
        if (!isOpen) return;
        panelRef.current?.focus();
        window.addEventListener('keydown', handleKey);
        return () => window.removeEventListener('keydown', handleKey);
    }, [isOpen, handleKey]);

    if (!isOpen) return null;

    return (
        <div className="migration-overlay" onClick={onCancel}>
            <div
                className="migration-panel"
                ref={panelRef}
                tabIndex={-1}
                onClick={(e) => e.stopPropagation()}
            >
                <div className="migration-header">
                    <h2>{title}</h2>
                    <button className="close-btn" onClick={onCancel}>
                        ×
                    </button>
                </div>

                <div className="migration-summary">
                    <span className="migration-summary-counts">
                        {summary.callsChanged} call
                        {summary.callsChanged === 1 ? '' : 's'} ·{' '}
                        {summary.assignmentsChanged} assignment
                        {summary.assignmentsChanged === 1 ? '' : 's'} ·{' '}
                        {summary.commentsChanged} comment
                        {summary.commentsChanged === 1 ? '' : 's'} rewritten
                    </span>
                    {summary.skippedVariables.length > 0 && (
                        <div className="migration-summary-warning">
                            {skippedLabel} {summary.skippedVariables.join(', ')}
                        </div>
                    )}
                    {summary.error && (
                        <div className="migration-summary-error">
                            Parse error: {summary.error}
                        </div>
                    )}
                </div>

                <div className="migration-body">
                    {noChanges ? (
                        <div className="migration-empty">
                            Buffer already migrated — no changes to apply.
                        </div>
                    ) : (
                        <DiffEditor
                            original={original}
                            modified={migrated}
                            language="javascript"
                            theme={monacoThemeId}
                            beforeMount={(monaco) => {
                                applyMonacoTheme(
                                    monaco,
                                    appTheme,
                                    monacoThemeId,
                                );
                            }}
                            options={{
                                readOnly: true,
                                renderSideBySide: true,
                                minimap: { enabled: false },
                                scrollBeyondLastLine: false,
                                fontFamily: font,
                                fontSize,
                                fontLigatures,
                            }}
                            height="100%"
                        />
                    )}
                </div>

                <div className="migration-footer">
                    <button className="btn btn-secondary" onClick={onCancel}>
                        Cancel
                    </button>
                    <button
                        className="btn btn-primary"
                        onClick={onApply}
                        disabled={!canApply}
                    >
                        Apply
                    </button>
                </div>
            </div>
        </div>
    );
}
