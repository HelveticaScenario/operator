import type { ReactNode } from 'react';
import React, { useState, useMemo, useEffect, useCallback } from 'react';
import Markdown from 'react-markdown';
import { VideoHelp } from './VideoHelp';
import electronAPI from '../electronAPI';
import type {
    DslTypeName,
    TypeDocumentation,
    GlobalFunctionDoc,
} from '../../shared/dsl/typeDocs';
import {
    TYPE_DOCS,
    DSL_TYPE_NAMES,
    isDslType,
    GLOBAL_DOCS,
} from '../../shared/dsl/typeDocs';
import type {
    EnumVariantInfo,
    Schema,
    Schemas,
} from '../../shared/dsl/schemaTypeResolver';
import {
    schemaToTypeExpr,
    getEnumVariants,
} from '../../shared/dsl/schemaTypeResolver';
import './HelpWindow.css';

type Page =
    | 'getting-started'
    | 'hotkeys'
    | 'globals'
    | 'types'
    | 'reference'
    | 'video';

/**
 * Regex pattern matching all DSL type names for linkification.
 * Sorted longest-first so `Poly<Signal>` matches before `Signal`.
 * Uses lookaround instead of \b since type names contain non-word chars (<>).
 */
const SORTED_TYPE_NAMES = [...DSL_TYPE_NAMES].sort(
    (a, b) => b.length - a.length,
);
const TYPE_PATTERN_SOURCE = `(?<!\\w)(${SORTED_TYPE_NAMES.join('|')})(?!\\w)`;

interface TypeLinkProps {
    typeName: DslTypeName;
    onTypeClick: (typeName: DslTypeName) => void;
}

/**
 * A clickable link that navigates to a type's documentation.
 */
const TypeLink: React.FC<TypeLinkProps> = ({ typeName, onTypeClick }) => (
    <button
        className="type-link"
        onClick={() => onTypeClick(typeName)}
        title={`View ${typeName} documentation`}
    >
        {typeName}
    </button>
);

interface LinkifyTypesProps {
    text: string;
    onTypeClick: (typeName: DslTypeName) => void;
}

/**
 * Component that renders text with DSL type names as clickable links.
 */
const LinkifyTypes: React.FC<LinkifyTypesProps> = ({ text, onTypeClick }) => {
    const parts: ReactNode[] = [];
    let lastIndex = 0;
    let match: RegExpExecArray | null;
    let key = 0;

    const typePattern = new RegExp(TYPE_PATTERN_SOURCE, 'g');

    while ((match = typePattern.exec(text)) !== null) {
        // Add text before the match
        if (match.index > lastIndex) {
            parts.push(text.slice(lastIndex, match.index));
        }
        // Add the type link
        const typeName = match[1] as DslTypeName;
        parts.push(
            <TypeLink
                key={key++}
                typeName={typeName}
                onTypeClick={onTypeClick}
            />,
        );
        lastIndex = match.index + match[0].length;
    }

    // Add remaining text
    if (lastIndex < text.length) {
        parts.push(text.slice(lastIndex));
    }

    return <>{parts}</>;
};

interface TypeCardProps {
    typeDoc: TypeDocumentation;
    isExpanded: boolean;
    onToggle: () => void;
    onTypeClick: (typeName: DslTypeName) => void;
}

/**
 * Card displaying documentation for a single DSL type.
 */
const TypeCard: React.FC<TypeCardProps> = ({
    typeDoc,
    isExpanded,
    onToggle,
    onTypeClick,
}) => (
    <div className={`type-card ${isExpanded ? 'expanded' : ''}`}>
        <div className="type-card-header" onClick={onToggle}>
            <h3>{typeDoc.name}</h3>
            <span className="expand-icon">{isExpanded ? '▼' : '▶'}</span>
        </div>

        <p className="type-description">
            <LinkifyTypes
                text={typeDoc.description}
                onTypeClick={onTypeClick}
            />
        </p>

        {isExpanded && (
            <div className="type-details">
                {typeDoc.definition && (
                    <div className="type-definition">
                        <h4>Definition</h4>
                        <code>{typeDoc.definition}</code>
                    </div>
                )}

                {typeDoc.examples.length > 0 && (
                    <div className="type-examples">
                        <h4>Examples</h4>
                        <pre>{typeDoc.examples.join('\n')}</pre>
                    </div>
                )}

                {typeDoc.methods && typeDoc.methods.length > 0 && (
                    <div className="type-methods">
                        <h4>Methods</h4>
                        {typeDoc.methods.map((method) => (
                            <div key={method.name} className="method-card">
                                <code className="method-signature">
                                    <LinkifyTypes
                                        text={method.signature}
                                        onTypeClick={onTypeClick}
                                    />
                                </code>
                                <p>
                                    <LinkifyTypes
                                        text={method.description}
                                        onTypeClick={onTypeClick}
                                    />
                                </p>
                                {method.example && (
                                    <pre className="method-example">
                                        {method.example}
                                    </pre>
                                )}
                            </div>
                        ))}
                    </div>
                )}

                {typeDoc.seeAlso.length > 0 && (
                    <div className="type-see-also">
                        <h4>See Also</h4>
                        <div className="see-also-links">
                            {typeDoc.seeAlso.map((typeName) =>
                                isDslType(typeName) ? (
                                    <TypeLink
                                        key={typeName}
                                        typeName={typeName}
                                        onTypeClick={onTypeClick}
                                    />
                                ) : (
                                    <span key={typeName}>{typeName}</span>
                                ),
                            )}
                        </div>
                    </div>
                )}
            </div>
        )}
    </div>
);

export const HelpWindow: React.FC = () => {
    const [activePage, setActivePage] = useState<Page>('hotkeys');
    const [searchQuery, setSearchQuery] = useState('');
    const [schemas, setSchemas] = useState<Schemas>();
    const [selectedType, setSelectedType] = useState<DslTypeName | null>(null);
    const [expandedTypes, setExpandedTypes] = useState(new Set<DslTypeName>());

    // Fetch schemas once at mount
    useEffect(() => {
        electronAPI
            .getSchemas()
            .then((schemaList) => {
                // _clock is internal-only (used for ROOT_CLOCK); hide from user-facing docs
                setSchemas(schemaList.filter((e) => !e.name.startsWith('_')));
            })
            .catch(console.error);
    }, []);

    // Listen for navigation events from main process (definition provider)
    useEffect(() => {
        const unsubscribe = electronAPI.onNavigateToSymbol?.((data) => {
            const { symbolType, symbolName } = data;

            if (symbolType === 'type' && isDslType(symbolName)) {
                // Navigate to types page and expand the type
                setActivePage('types');
                setSelectedType(symbolName);
                setExpandedTypes((prev) => new Set(prev).add(symbolName));
            } else if (symbolType === 'module' || symbolType === 'namespace') {
                // Navigate to reference page and search for the module
                setActivePage('reference');
                setSearchQuery(symbolName);
            }
        });

        return () => unsubscribe?.();
    }, []);

    // Handle type link clicks - navigate to types page and expand the type
    const handleTypeClick = useCallback((typeName: DslTypeName) => {
        setActivePage('types');
        setSelectedType(typeName);
        setExpandedTypes((prev) => new Set(prev).add(typeName));
    }, []);

    // Toggle type card expansion
    const toggleTypeExpanded = useCallback((typeName: DslTypeName) => {
        setExpandedTypes((prev) => {
            const next = new Set(prev);
            if (next.has(typeName)) {
                next.delete(typeName);
            } else {
                next.add(typeName);
            }
            return next;
        });
    }, []);

    // Scroll to selected type when it changes
    useEffect(() => {
        if (selectedType && activePage === 'types') {
            const element = document.getElementById(`type-${selectedType}`);
            if (element) {
                element.scrollIntoView({ behavior: 'smooth', block: 'start' });
            }
        }
    }, [selectedType, activePage]);

    const filteredModules = useMemo(() => {
        if (!schemas) {
            return [];
        }
        return Object.values(schemas).filter(
            (schema) =>
                schema.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
                schema.documentation
                    .toLowerCase()
                    .includes(searchQuery.toLowerCase()),
        );
    }, [schemas, searchQuery]);

    const getSignature = (module: Schema): string => {
        const { positionalArgs } = module;
        const positionalKeys = new Set(positionalArgs.map((a) => a.name));
        const schemaRequired: string[] = module.paramsSchema.required ?? [];

        const parts: string[] = positionalArgs.map((a) =>
            schemaRequired.includes(a.name) ? a.name : `${a.name}?`,
        );

        const configKeys = Object.keys(module.paramsSchema.properties).filter(
            (k) => !positionalKeys.has(k),
        );

        if (configKeys.length > 0) {
            parts.push(
                `{ ${configKeys.map((k) => (schemaRequired.includes(k) ? k : `${k}?`)).join(', ')} }`,
            );
        }

        return `${module.name}(${parts.join(', ')})`;
    };

    const getParams = (module: Schema) => {
        const properties = Object.entries(module.paramsSchema.properties);
        return properties.map(([name, schema]) => {
            let type: string | undefined;
            try {
                type = schemaToTypeExpr(schema, module.paramsSchema);
            } catch {
                // Fall back to no type annotation for unsupported schemas
            }
            let variants: EnumVariantInfo[] | null = null;
            try {
                variants = getEnumVariants(schema, module.paramsSchema);
            } catch {
                // Fall back to no variant info
            }
            return {
                description: schema.description as string | undefined,
                name,
                type,
                variants,
            };
        });
    };

    const renderContent = () => {
        switch (activePage) {
            case 'globals': {
                const groups = GLOBAL_DOCS.reduce<
                    Record<string, GlobalFunctionDoc[]>
                >((acc, fn) => {
                    (acc[fn.group] ??= []).push(fn);
                    return acc;
                }, {});
                return (
                    <div>
                        <h2>Global Functions</h2>
                        <p className="types-intro">
                            These functions are available in every patch without
                            any imports.
                        </p>
                        {Object.entries(groups).map(([group, fns]) => (
                            <div key={group}>
                                <h3>{group}</h3>
                                {fns.map((fn) => (
                                    <div key={fn.name} className="module-card">
                                        <h4>
                                            <code>{fn.signature}</code>
                                        </h4>
                                        <p>{fn.description}</p>
                                        {fn.params && fn.params.length > 0 && (
                                            <>
                                                <h5>Parameters</h5>
                                                <ul>
                                                    {fn.params.map((p) => (
                                                        <li key={p.name}>
                                                            <strong>
                                                                {p.name}:{' '}
                                                                <LinkifyTypes
                                                                    text={
                                                                        p.type
                                                                    }
                                                                    onTypeClick={
                                                                        handleTypeClick
                                                                    }
                                                                />
                                                            </strong>{' '}
                                                            &mdash;{' '}
                                                            {p.description}
                                                        </li>
                                                    ))}
                                                </ul>
                                            </>
                                        )}
                                        {fn.examples.length > 0 && (
                                            <>
                                                <h5>Example</h5>
                                                <pre>
                                                    {fn.examples.join('\n\n')}
                                                </pre>
                                            </>
                                        )}
                                    </div>
                                ))}
                            </div>
                        ))}
                    </div>
                );
            }
            case 'video':
                return <VideoHelp />;
            case 'hotkeys':
                return (
                    <div>
                        <h2>Hotkeys</h2>
                        <h3>Patch Execution</h3>
                        <ul>
                            <li>
                                <b>Ctrl + Enter</b>: Update Patch (queued for
                                next bar)
                            </li>
                            <li>
                                <b>Ctrl + Shift + Enter</b>: Update Patch
                                (queued for next beat)
                            </li>
                            <li>
                                <b>Ctrl + .</b>: Stop Sound
                            </li>
                            <li>
                                <b>Ctrl + \</b>: Cancel Queued Update (the
                                playing patch keeps running)
                            </li>
                        </ul>
                        <p style={{ fontSize: '0.9em', opacity: 0.7 }}>
                            Pressing Ctrl+Enter again while an update is already
                            queued will discard the old update and apply the new
                            one immediately.
                        </p>
                        <h3>Files</h3>
                        <ul>
                            <li>
                                <b>Cmd/Ctrl + S</b>: Save
                            </li>
                            <li>
                                <b>Cmd/Ctrl + O</b>: Open Workspace
                            </li>
                            <li>
                                <b>Cmd/Ctrl + N</b>: New File
                            </li>
                            <li>
                                <b>Cmd/Ctrl + W</b>: Close Buffer
                            </li>
                        </ul>
                    </div>
                );
            case 'types':
                return (
                    <div>
                        <h2>Type Reference</h2>
                        <p className="types-intro">
                            These are the core types used in the modular DSL.
                            Click on any type name throughout the documentation
                            to navigate to its definition.
                        </p>

                        <div className="types-grid">
                            {DSL_TYPE_NAMES.map((typeName) => {
                                const typeDoc = TYPE_DOCS[typeName];
                                return (
                                    <div key={typeName} id={`type-${typeName}`}>
                                        <TypeCard
                                            typeDoc={typeDoc}
                                            isExpanded={expandedTypes.has(
                                                typeName,
                                            )}
                                            onToggle={() =>
                                                toggleTypeExpanded(typeName)
                                            }
                                            onTypeClick={handleTypeClick}
                                        />
                                    </div>
                                );
                            })}
                        </div>
                    </div>
                );
            case 'reference':
                return (
                    <div>
                        <h2 className="help-title">Module Reference</h2>
                        <div className="search-container">
                            <input
                                type="text"
                                placeholder="Search modules..."
                                value={searchQuery}
                                onChange={(e) => setSearchQuery(e.target.value)}
                                className="search-input"
                            />
                        </div>
                        {filteredModules.map((module) => {
                            // Console.log(module);
                            const params = getParams(module);
                            return (
                                <div key={module.name} className="module-card">
                                    <h3>
                                        <code>{getSignature(module)}</code>
                                    </h3>
                                    <div className="module-documentation">
                                        <Markdown>
                                            {module.documentation}
                                        </Markdown>
                                    </div>
                                    <h4>Inputs</h4>
                                    <ul>
                                        {params.map(
                                            ({
                                                name,
                                                type,
                                                description,
                                                variants,
                                            }) => (
                                                <li key={name}>
                                                    <strong>
                                                        {name}
                                                        {type && (
                                                            <>
                                                                {': '}
                                                                <LinkifyTypes
                                                                    text={type}
                                                                    onTypeClick={
                                                                        handleTypeClick
                                                                    }
                                                                />
                                                            </>
                                                        )}
                                                    </strong>
                                                    {description && (
                                                        <>
                                                            {' '}
                                                            &mdash;{' '}
                                                            {description}
                                                        </>
                                                    )}
                                                    {variants &&
                                                        variants.some(
                                                            (v) =>
                                                                v.description,
                                                        ) &&
                                                        (variants.length > 8 ? (
                                                            <details className="enum-variants">
                                                                <summary>
                                                                    {
                                                                        variants.length
                                                                    }{' '}
                                                                    values
                                                                    (click to
                                                                    expand)
                                                                </summary>
                                                                <ul>
                                                                    {variants.map(
                                                                        (v) => (
                                                                            <li
                                                                                key={
                                                                                    v.value
                                                                                }
                                                                            >
                                                                                <code>
                                                                                    {
                                                                                        v.rawValue as string
                                                                                    }
                                                                                </code>
                                                                                {v.description && (
                                                                                    <>
                                                                                        {' '}
                                                                                        &mdash;{' '}
                                                                                        {
                                                                                            v.description
                                                                                        }
                                                                                    </>
                                                                                )}
                                                                            </li>
                                                                        ),
                                                                    )}
                                                                </ul>
                                                            </details>
                                                        ) : (
                                                            <ul className="enum-variants">
                                                                {variants.map(
                                                                    (v) => (
                                                                        <li
                                                                            key={
                                                                                v.value
                                                                            }
                                                                        >
                                                                            <code>
                                                                                {
                                                                                    v.rawValue as string
                                                                                }
                                                                            </code>
                                                                            {v.description && (
                                                                                <>
                                                                                    {' '}
                                                                                    &mdash;{' '}
                                                                                    {
                                                                                        v.description
                                                                                    }
                                                                                </>
                                                                            )}
                                                                        </li>
                                                                    ),
                                                                )}
                                                            </ul>
                                                        ))}
                                                </li>
                                            ),
                                        )}
                                    </ul>
                                    <h4>Outputs</h4>
                                    <ul>
                                        {module.outputs.map((out) => (
                                            <li key={out.name}>
                                                <strong>
                                                    {out.default
                                                        ? 'main'
                                                        : out.name}
                                                </strong>
                                                :{' '}
                                                <LinkifyTypes
                                                    text={out.description}
                                                    onTypeClick={
                                                        handleTypeClick
                                                    }
                                                />
                                            </li>
                                        ))}
                                    </ul>
                                </div>
                            );
                        })}
                    </div>
                );
        }
    };

    return (
        <div className="help-window">
            <div className="sidebar">
                <button
                    className={activePage === 'hotkeys' ? 'active' : ''}
                    onClick={() => setActivePage('hotkeys')}
                >
                    Hotkeys
                </button>
                <button
                    className={activePage === 'globals' ? 'active' : ''}
                    onClick={() => setActivePage('globals')}
                >
                    Globals
                </button>
                <button
                    className={activePage === 'types' ? 'active' : ''}
                    onClick={() => setActivePage('types')}
                >
                    Types
                </button>
                <button
                    className={activePage === 'reference' ? 'active' : ''}
                    onClick={() => setActivePage('reference')}
                >
                    Reference
                </button>
                <button
                    className={activePage === 'video' ? 'active' : ''}
                    onClick={() => setActivePage('video')}
                >
                    Video
                </button>
            </div>
            <div className="content">{renderContent()}</div>
        </div>
    );
};
