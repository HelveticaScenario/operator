import { DiffEditor } from '@monaco-editor/react';
import { useEffect, useRef } from 'react';

import { useTheme } from '../themes/ThemeContext';
import { applyMonacoTheme } from './monaco/theme';
import './SaveConflict.css';

const baseName = (filePath: string) => filePath.split(/[/\\]/).pop() ?? '';

interface NotificationProps {
    filePath: string;
    onCompare: () => void;
    onOverwrite: () => void;
    onDismiss: () => void;
}

/** Non-modal notice for a save refused because the file changed on disk. */
export function SaveConflictNotification({
    filePath,
    onCompare,
    onOverwrite,
    onDismiss,
}: NotificationProps) {
    return (
        <div className="save-conflict-notification" role="alert">
            <div className="save-conflict-notification__row">
                <span className="save-conflict-notification__message">
                    Failed to save &apos;{baseName(filePath)}&apos;: The content
                    of the file is newer. Please compare your version with the
                    file contents or overwrite the content of the file with your
                    changes.
                </span>
                <button
                    className="save-conflict__icon-btn"
                    onClick={onDismiss}
                    title="Close"
                    aria-label="Close"
                >
                    ×
                </button>
            </div>
            <div className="save-conflict-notification__actions">
                <button
                    className="save-conflict__btn save-conflict__btn--primary"
                    onClick={onCompare}
                >
                    Compare
                </button>
                <button className="save-conflict__btn" onClick={onOverwrite}>
                    Overwrite
                </button>
            </div>
        </div>
    );
}

interface DiffProps {
    filePath: string;
    diskContent: string;
    bufferContent: string;
    onAcceptLocal: () => void;
    onRevertLocal: () => void;
    onClose: () => void;
}

/** Disk content (left) against the unsaved buffer (right). */
export function SaveConflictDiff({
    filePath,
    diskContent,
    bufferContent,
    onAcceptLocal,
    onRevertLocal,
    onClose,
}: DiffProps) {
    const panelRef = useRef<HTMLDivElement>(null);
    const { theme: appTheme, font, fontLigatures, fontSize } = useTheme();
    const monacoThemeId = `theme-${appTheme.id}`;
    const name = baseName(filePath);

    useEffect(() => {
        panelRef.current?.focus();
        const handleKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                onClose();
            }
        };
        window.addEventListener('keydown', handleKey);
        return () => window.removeEventListener('keydown', handleKey);
    }, [onClose]);

    return (
        <div className="save-conflict-overlay" onClick={onClose}>
            <div
                className="save-conflict-panel"
                ref={panelRef}
                tabIndex={-1}
                onClick={(e) => e.stopPropagation()}
            >
                <div className="save-conflict-header">
                    <h2>
                        {name} (in file) ↔ {name} (in Operator) - Resolve save
                        conflict
                    </h2>
                    <button
                        className="save-conflict__icon-btn"
                        onClick={onClose}
                        title="Close"
                        aria-label="Close"
                    >
                        ×
                    </button>
                </div>
                <div className="save-conflict-choices">
                    <div className="save-conflict-choice">
                        <span>File on disk</span>
                        <button
                            className="save-conflict__btn"
                            onClick={onRevertLocal}
                            title="Discard your changes and revert to file contents"
                        >
                            Use file contents
                        </button>
                    </div>
                    <div className="save-conflict-choice">
                        <span>Your changes</span>
                        <button
                            className="save-conflict__btn save-conflict__btn--primary"
                            onClick={onAcceptLocal}
                            title="Use your changes and overwrite file contents"
                        >
                            Use your changes
                        </button>
                    </div>
                </div>
                <div className="save-conflict-body">
                    <DiffEditor
                        original={diskContent}
                        modified={bufferContent}
                        language={
                            filePath.endsWith('.json') ? 'json' : 'javascript'
                        }
                        theme={monacoThemeId}
                        beforeMount={(monaco) => {
                            applyMonacoTheme(monaco, appTheme, monacoThemeId);
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
                </div>
            </div>
        </div>
    );
}
