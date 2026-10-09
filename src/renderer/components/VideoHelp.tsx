import React from 'react';
import {
    VIDEO_CHAIN,
    VIDEO_CHAIN_EXAMPLES,
    VIDEO_CHAIN_INTRO,
    VIDEO_DOCS,
    VIDEO_GROUPS,
    VIDEO_INTRO,
} from '../../shared/dsl/videoDocs';

/** Splits docs text into paragraphs and renders `code` spans. */
const Prose: React.FC<{ text: string }> = ({ text }) => (
    <>
        {text.split('\n\n').map((paragraph) => (
            <p key={paragraph}>
                {paragraph
                    .split(/(`[^`]+`)/)
                    .map((part, i) =>
                        part.startsWith('`') ? (
                            <code key={i}>{part.slice(1, -1)}</code>
                        ) : (
                            part
                        ),
                    )}
            </p>
        ))}
    </>
);

/** The Help window's Video page, rendered from {@link VIDEO_DOCS}. */
export const VideoHelp: React.FC = () => (
    <div>
        <h2>Video</h2>
        <div className="types-intro">
            <Prose text={VIDEO_INTRO.description} />
            <pre>{VIDEO_INTRO.examples.join('\n\n')}</pre>
        </div>
        <h3>Chaining</h3>
        <div className="module-card">
            <Prose text={VIDEO_CHAIN_INTRO} />
            <pre>{VIDEO_CHAIN_EXAMPLES.join('\n\n')}</pre>
            <h5>Methods</h5>
            <ul>
                {VIDEO_CHAIN.map((method) => (
                    <li key={method.name}>
                        <strong>{method.name}</strong> &mdash;{' '}
                        {method.description}
                    </li>
                ))}
            </ul>
        </div>
        {VIDEO_GROUPS.map((group) => (
            <div key={group}>
                <h3>{group}</h3>
                {VIDEO_DOCS.filter((doc) => doc.group === group).map((doc) => (
                    <div key={doc.name} className="module-card">
                        <h4>
                            <code>$v.{doc.name}</code>
                        </h4>
                        <pre>{doc.declarations.join('\n')}</pre>
                        <Prose text={doc.description} />
                        {doc.params.length > 0 && (
                            <>
                                <h5>Parameters</h5>
                                <ul>
                                    {doc.params.map((param) => (
                                        <li key={param.name}>
                                            <strong>{param.name}</strong>{' '}
                                            &mdash;{' '}
                                            {param.description
                                                .split(/(`[^`]+`)/)
                                                .map((part, i) =>
                                                    part.startsWith('`') ? (
                                                        <code key={i}>
                                                            {part.slice(1, -1)}
                                                        </code>
                                                    ) : (
                                                        part
                                                    ),
                                                )}
                                        </li>
                                    ))}
                                </ul>
                            </>
                        )}
                        {doc.examples.length > 0 && (
                            <>
                                <h5>Example</h5>
                                <pre>{doc.examples.join('\n\n')}</pre>
                            </>
                        )}
                    </div>
                ))}
            </div>
        ))}
    </div>
);
