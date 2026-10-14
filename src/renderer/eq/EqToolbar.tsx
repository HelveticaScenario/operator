import React from 'react';

import {
    DEFAULT_Q,
    type EqSectionType,
    EQ_SECTION_TYPES,
    MAX_EQ_CHANNEL_LISTS,
    MAX_EQ_SECTIONS,
    typeHasGain,
} from './eqMath';
import { SECTION_COLORS } from './eqGeometry';
import { isCodeOnly } from './eqGestures';
import { FREQ_FIELD, GAIN_FIELD, Q_FIELD, Readout } from './EqReadout';
import type { EqPanelProps } from './EqPanel';

const TYPE_LABELS: Record<EqSectionType, string> = {
    highShelf: 'High shelf',
    highpass: 'Highpass',
    lowShelf: 'Low shelf',
    lowpass: 'Lowpass',
    notch: 'Notch',
    peak: 'Peak',
};

const TYPE_GLYPHS: Record<EqSectionType, string> = {
    highShelf: 'M1 10 H6 C9 10 10 4 13 4 H19',
    highpass: 'M3 12 C5 6 7 5 10 5 H19',
    lowShelf: 'M1 4 H6 C9 4 10 10 13 10 H19',
    lowpass: 'M1 5 H10 C13 5 15 6 17 12',
    notch: 'M1 5 H7 L10 12 L13 5 H19',
    peak: 'M1 10 C6 10 7 3 10 3 C13 3 14 10 19 10',
};

/** L/R for a stereo pair of lists, channel numbers otherwise. */
function channelLabel(index: number, count: number): string {
    if (count === 2) {
        return index === 0 ? 'L' : 'R';
    }
    return String(index + 1);
}

type EqToolbarProps = Pick<
    EqPanelProps,
    | 'sections'
    | 'channelLists'
    | 'channel'
    | 'ghosts'
    | 'selected'
    | 'editable'
    | 'onSelect'
    | 'onChannel'
    | 'onChange'
    | 'onAdd'
    | 'onRemove'
    | 'onAddChannel'
    | 'onRemoveChannel'
    | 'onRevert'
>;

/** Channel tabs, section chips, and the selected section's type and values. */
export function EqToolbar({
    sections,
    channelLists,
    channel,
    ghosts,
    selected,
    editable,
    onSelect,
    onChannel,
    onChange,
    onAdd,
    onRemove,
    onAddChannel,
    onRemoveChannel,
    onRevert,
}: EqToolbarProps): React.ReactElement {
    const sel = selected !== null ? sections[selected] : undefined;
    const selGhost = selected !== null ? (ghosts[selected] ?? null) : null;

    const setType = (type: EqSectionType, codeOnly: boolean) => {
        if (selected === null || (codeOnly && !editable)) {
            return;
        }
        onChange(selected, { type }, 'commit', codeOnly);
    };

    return (
        <div className="eq-panel-toolbar">
            <div className="eq-channels" role="tablist">
                {channelLists ? (
                    channelLists.map((_, i) => (
                        <span key={i} className="eq-removable">
                            <button
                                type="button"
                                role="tab"
                                aria-selected={i === channel}
                                className={
                                    i === channel
                                        ? 'eq-channel eq-channel-active'
                                        : 'eq-channel'
                                }
                                title={`Edit channel ${i + 1}'s sections`}
                                onClick={() => onChannel(i)}
                            >
                                {channelLabel(i, channelLists.length)}
                            </button>
                            <button
                                type="button"
                                className="eq-remove-x"
                                title={`Remove channel ${i + 1}'s list`}
                                onClick={() => onRemoveChannel(i)}
                            >
                                ×
                            </button>
                        </span>
                    ))
                ) : (
                    <button
                        type="button"
                        role="tab"
                        aria-selected
                        className="eq-channel eq-channel-active"
                        title="One section list for every channel"
                    >
                        all
                    </button>
                )}
                <button
                    type="button"
                    className="eq-channel"
                    title={
                        channelLists
                            ? 'Add a channel list, copying this one'
                            : 'Split into per-channel lists (L/R)'
                    }
                    disabled={
                        (channelLists?.length ?? 0) >= MAX_EQ_CHANNEL_LISTS
                    }
                    onClick={onAddChannel}
                >
                    +
                </button>
            </div>
            <div className="eq-chips">
                {sections.map((s, i) => (
                    <span key={i} className="eq-removable">
                        <button
                            type="button"
                            className={
                                i === selected
                                    ? 'eq-chip eq-chip-selected'
                                    : 'eq-chip'
                            }
                            style={
                                {
                                    '--chip-color':
                                        SECTION_COLORS[
                                            i % SECTION_COLORS.length
                                        ],
                                } as React.CSSProperties
                            }
                            title={`Section ${i + 1}: ${TYPE_LABELS[s.type]}`}
                            onClick={() => onSelect(i === selected ? null : i)}
                        >
                            {i + 1}
                        </button>
                        <button
                            type="button"
                            className="eq-remove-x"
                            title={`Remove section ${i + 1}`}
                            onClick={() => onRemove(i)}
                        >
                            ×
                        </button>
                    </span>
                ))}
                <button
                    type="button"
                    className="eq-chip eq-chip-add"
                    title="Add a section (or double-click the graph)"
                    disabled={sections.length >= MAX_EQ_SECTIONS}
                    onClick={() => onAdd(1000, 0)}
                >
                    +
                </button>
            </div>
            {sel ? (
                <>
                    <div className="eq-types" role="radiogroup">
                        {EQ_SECTION_TYPES.map((t) => {
                            const classes = ['eq-type'];
                            if (sel.type === t) {
                                classes.push('eq-type-active');
                            }
                            if (
                                selGhost &&
                                selGhost.type === t &&
                                selGhost.type !== sel.type
                            ) {
                                classes.push('eq-type-ghost');
                            }
                            return (
                                <button
                                    key={t}
                                    type="button"
                                    role="radio"
                                    aria-checked={sel.type === t}
                                    className={classes.join(' ')}
                                    title={`${TYPE_LABELS[t]} (⌃/⌘-click: edit code only)`}
                                    onClick={(e) => setType(t, isCodeOnly(e))}
                                    onContextMenu={(e) => {
                                        e.preventDefault();
                                        if (e.ctrlKey) {
                                            setType(t, true);
                                        }
                                    }}
                                >
                                    <svg
                                        width="20"
                                        height="14"
                                        viewBox="0 0 20 14"
                                    >
                                        <path d={TYPE_GLYPHS[t]} />
                                    </svg>
                                </button>
                            );
                        })}
                    </div>
                    <div className="eq-readouts">
                        <Readout
                            field={FREQ_FIELD}
                            value={sel.freqHz}
                            ghost={selGhost?.freqHz ?? null}
                            driven={sel.driven.freq}
                            editable={editable}
                            onRevert={() => onRevert(selected!, ['freqHz'])}
                            onChange={(v, phase, codeOnly) =>
                                onChange(
                                    selected!,
                                    v === null ? {} : { freqHz: v },
                                    phase,
                                    codeOnly,
                                )
                            }
                        />
                        {typeHasGain(sel.type) && (
                            <Readout
                                field={GAIN_FIELD}
                                value={sel.gainDb}
                                ghost={selGhost?.gainDb ?? null}
                                driven={sel.driven.gain}
                                editable={editable}
                                onReset={() =>
                                    onChange(
                                        selected!,
                                        { gainDb: 0 },
                                        'commit',
                                        false,
                                    )
                                }
                                onRevert={() => onRevert(selected!, ['gainDb'])}
                                onChange={(v, phase, codeOnly) =>
                                    onChange(
                                        selected!,
                                        v === null ? {} : { gainDb: v },
                                        phase,
                                        codeOnly,
                                    )
                                }
                            />
                        )}
                        <Readout
                            field={Q_FIELD}
                            value={sel.q}
                            ghost={selGhost?.q ?? null}
                            driven={sel.driven.q}
                            editable={editable}
                            onReset={() =>
                                onChange(
                                    selected!,
                                    { q: DEFAULT_Q },
                                    'commit',
                                    false,
                                )
                            }
                            onRevert={() => onRevert(selected!, ['q'])}
                            onChange={(v, phase, codeOnly) =>
                                onChange(
                                    selected!,
                                    v === null ? {} : { q: v },
                                    phase,
                                    codeOnly,
                                )
                            }
                        />
                    </div>
                    <button
                        type="button"
                        className="eq-remove"
                        title="Remove section (Delete)"
                        onClick={() => onRemove(selected!)}
                    >
                        ×
                    </button>
                </>
            ) : (
                <div className="eq-hint">
                    {sections.length === 0
                        ? 'double-click the graph or press + to add a section'
                        : 'drag a node · select it, then drag its handles or scroll for Q · double-click to add · right-click resets · ⌃/⌘ edits code only, ⌃/⌘ right-click reverts it'}
                </div>
            )}
        </div>
    );
}
