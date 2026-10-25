import { useEffect, useMemo, type ReactNode } from 'react';
import Markdown, { type Components } from 'react-markdown';
import {
    combineReleaseNotes,
    type ReleaseNotes,
} from '../../shared/releaseNotes';
import './ReleaseNotesDialog.css';

export interface ReleaseNotesAction {
    label: string;
    onClick: () => void;
    primary?: boolean;
}

interface Props {
    title: string;
    /** One line under the title, such as the installed and offered versions. */
    summary?: ReactNode;
    /** Newest first. */
    releases: ReleaseNotes[];
    /** Footer buttons; a lone Close button when omitted. */
    actions?: ReleaseNotesAction[];
    onClose: () => void;
}

const KIND_ORDER = ['New', 'Improved', 'Fixed'] as const;

/**
 * Release bodies come from the network, so raw HTML is dropped, and links
 * open with target=_blank, which the main window's open handler sends to the
 * system browser instead of navigating the app.
 */
const MARKDOWN_COMPONENTS: Components = {
    a: ({ href, children }) => (
        <a href={href} target="_blank" rel="noreferrer noopener">
            {children}
        </a>
    ),
};

function NoteMarkdown({ text }: { text: string }) {
    return (
        <Markdown skipHtml components={MARKDOWN_COMPONENTS}>
            {text}
        </Markdown>
    );
}

export function ReleaseNotesDialog({
    title,
    summary,
    releases,
    actions,
    onClose,
}: Props) {
    const combined = useMemo(() => combineReleaseNotes(releases), [releases]);
    const footer = actions ?? [
        { label: 'Close', onClick: onClose, primary: true },
    ];

    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                event.stopPropagation();
                onClose();
            }
        };
        window.addEventListener('keydown', onKeyDown, true);
        return () => window.removeEventListener('keydown', onKeyDown, true);
    }, [onClose]);

    const isEmpty =
        combined.breaking.length === 0 && combined.releases.length === 0;

    return (
        <div
            className="release-notes-overlay"
            onMouseDown={(event) => {
                if (event.target === event.currentTarget) onClose();
            }}
        >
            <div
                className="release-notes-panel"
                role="dialog"
                aria-modal="true"
                aria-labelledby="release-notes-title"
            >
                <header className="release-notes-header">
                    <div>
                        <h2 id="release-notes-title">{title}</h2>
                        {summary && (
                            <p className="release-notes-summary">{summary}</p>
                        )}
                    </div>
                    <button
                        type="button"
                        className="release-notes-close"
                        aria-label="Close"
                        onClick={onClose}
                    >
                        ×
                    </button>
                </header>

                <div className="release-notes-body">
                    {isEmpty && (
                        <p className="release-notes-empty">
                            There are no release notes to show yet.
                        </p>
                    )}

                    {combined.breaking.length > 0 && (
                        <section className="release-notes-breaking">
                            <h3>Breaking changes</h3>
                            <p className="release-notes-breaking-intro">
                                Some of your patches may need small edits to
                                keep sounding the same.
                            </p>
                            <ul>
                                {combined.breaking.map((change, index) => (
                                    <li key={`${change.version}-${index}`}>
                                        <span className="release-notes-version-tag">
                                            {change.version}
                                        </span>
                                        <div className="release-notes-entry">
                                            <NoteMarkdown text={change.entry} />
                                        </div>
                                    </li>
                                ))}
                            </ul>
                        </section>
                    )}

                    {combined.releases.map((release) => (
                        <section
                            key={release.version}
                            className="release-notes-release"
                        >
                            <h3>
                                Version {release.version}
                                {release.date && (
                                    <span className="release-notes-date">
                                        {release.date}
                                    </span>
                                )}
                            </h3>
                            {KIND_ORDER.map((kind) => {
                                const entries = release.sections[kind];
                                if (!entries?.length) return null;
                                return (
                                    <div
                                        key={kind}
                                        className={`release-notes-kind release-notes-kind--${kind.toLowerCase()}`}
                                    >
                                        <h4>{kind}</h4>
                                        <ul>
                                            {entries.map((entry, index) => (
                                                <li key={index}>
                                                    <div className="release-notes-entry">
                                                        <NoteMarkdown
                                                            text={entry}
                                                        />
                                                    </div>
                                                </li>
                                            ))}
                                        </ul>
                                    </div>
                                );
                            })}
                        </section>
                    ))}
                </div>

                <footer className="release-notes-actions">
                    {footer.map((action) => (
                        <button
                            key={action.label}
                            type="button"
                            className={
                                action.primary
                                    ? 'release-notes-btn release-notes-btn--primary'
                                    : 'release-notes-btn'
                            }
                            onClick={action.onClick}
                            autoFocus={action.primary}
                        >
                            {action.label}
                        </button>
                    ))}
                </footer>
            </div>
        </div>
    );
}
