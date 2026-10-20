/**
 * Per-panel runtime state and the context the panel operations run in.
 */
import type { Root } from 'react-dom/client';
import type { editor } from 'monaco-editor';

import type { EqPanelDef } from '../../shared/dsl/eqPanelTypes';
import type { CodeStyle } from '../dsl/objectPropertyInsert';
import type { EqChangePhase } from './eqGestures';
import { quantizeGain, quantizeHz, quantizeQ } from './eqMath';
import {
    type EqField,
    type EqSectionChange,
    type EqSectionView,
    type LiveSectionValues,
    type RawEqParams,
    decodeSection,
    isPerChannel,
    sectionIsModulated,
    sectionLists,
    withLive,
} from './eqPanelModel';
import type { EqPanelSetup } from './eqPanelSetup';
import type { FieldLiteral } from './eqSourceLocate';
import type { SpectrumAnalyzer } from './eqSpectrum';

/** Code-side values of a section that differ from the audio. */
export type CodeGhost = EqSectionChange;

/** Live values per list and section; a section may not have reported yet. */
export type LiveValues = (LiveSectionValues | undefined)[][];

export interface Zone {
    id: string;
    delegate: editor.IViewZone;
    root: Root;
}

/**
 * The latest change to one section from the panel, kept so a queued patch
 * run (compiled before the change) can be brought up to date on commit.
 */
export interface LoggedChange {
    seq: number;
    list: number;
    index: number;
    change: EqSectionChange;
    codeOnly: boolean;
}

export interface PanelCallbacks {
    subscribeFrames: (cb: () => void) => () => void;
    onSelect: (index: number | null) => void;
    onChannel: (list: number) => void;
    onChange: (
        index: number,
        change: EqSectionChange,
        phase: EqChangePhase,
        codeOnly: boolean,
    ) => void;
    onAdd: (freqHz: number, gainDb: number) => void;
    onRemove: (index: number) => void;
    onAddChannel: () => void;
    onRemoveChannel: (list: number) => void;
    onRevert: (index: number, fields: (keyof EqSectionChange)[] | null) => void;
}

export interface PanelRuntime {
    def: EqPanelDef;
    /** Index into the setup's decorations. */
    index: number;
    params: RawEqParams;
    /** Any section param is signal-driven (the panel then polls live values). */
    modulated: boolean;
    /** The channel list being edited (always 0 in the shared form). */
    channel: number;
    /** Keyed by `sectionKey(list, index)`. */
    ghosts: Map<string, CodeGhost>;
    selected: number | null;
    notice: string | null;
    spectrum: SpectrumAnalyzer;
    /** Scope buffer key per channel list. */
    spectrumKeys: string[];
    /** The engine's running values per list and section, when polled. */
    live: LiveValues | null;
    frameListeners: Set<() => void>;
    zone: Zone | null;
    /** Source literals awaiting the next throttled write, keyed like ghosts. */
    pendingCode: Map<string, Partial<Record<EqField, FieldLiteral>>>;
    codeTimer: number | null;
    lastCodeWrite: number;
    engineFrame: number | null;
    /** Latest change per section and gesture kind, keyed
     *  `list:index:codeOnly`. */
    changeLog: Map<string, LoggedChange>;
    /** Sequence number of the last add/remove of a section or list. */
    structureSeq: number;
    callbacks: PanelCallbacks;
}

/** What the panel operations need from their owner. */
export interface EqPanelContext {
    readonly setup: EqPanelSetup | null;
    readonly codeStyle: CodeStyle | null;
    readonly editor: editor.IStandaloneCodeEditor | null;
    setModuleParam: (
        moduleId: string,
        moduleType: string,
        params: unknown,
    ) => Promise<unknown>;
    render(panel: PanelRuntime): void;
    /** A fresh, increasing panel edit sequence number. */
    nextEditSeq(): number;
}

export const NOTICES = {
    missing: 'call not found in the code — run to resync',
    queued: 'edited while the patch was queued — run to resync',
    skipped: 'non-literal values stay live only',
    stale: 'code changed by hand — run to resync',
    unsupported: 'sections are not an array literal — live only',
};

export function sectionKey(list: number, index: number): string {
    return `${list}:${index}`;
}

export function parseSectionKey(key: string): { list: number; index: number } {
    const [list, index] = key.split(':').map(Number);
    return { index, list };
}

export const FIELD_OF: Record<keyof EqSectionChange, EqField> = {
    freqHz: 'freq',
    gainDb: 'gain',
    q: 'q',
    type: 'type',
};

/** A section's live value for `k`, quantized like a written value. */
export function quantizedLive(
    view: EqSectionView,
    k: keyof EqSectionChange,
): EqSectionChange[keyof EqSectionChange] | null {
    switch (k) {
        case 'type':
            return view.type;
        case 'freqHz':
            return view.freqHz === null ? null : quantizeHz(view.freqHz);
        case 'gainDb':
            return view.gainDb === null ? null : quantizeGain(view.gainDb);
        case 'q':
            return view.q === null ? null : quantizeQ(view.q);
    }
}

/**
 * A poll's live values, with each section the poll lacks keeping its last
 * known value. A patch run starts the engine's live slot blank until the
 * next audio callback fills it, and a poll landing in that gap would
 * otherwise blank modulated sections.
 */
export function mergeLive(
    panel: PanelRuntime,
    next: LiveValues | null,
): LiveValues | null {
    if (!next) {
        return panel.live;
    }
    return sectionLists(panel.params).map((list, l) =>
        list.map((_, i) => next[l]?.[i] ?? panel.live?.[l]?.[i]),
    );
}

/** Every list's sections as shown: params plus live driven values. */
export function shownLists(panel: PanelRuntime): EqSectionView[][] {
    return sectionLists(panel.params).map((list, l) =>
        list.map((raw, i) =>
            withLive(decodeSection(raw), panel.live?.[l]?.[i]),
        ),
    );
}

export function paramsAreModulated(params: RawEqParams): boolean {
    return sectionLists(params).some((l) =>
        l.some((raw) => sectionIsModulated(decodeSection(raw))),
    );
}

/** Section counts per list, comparable across runs. */
export function shapeOf(params: RawEqParams): string {
    const lists = sectionLists(params).map((l) => l.length);
    return `${isPerChannel(params) ? 'per' : 'shared'}:${lists.join(',')}`;
}

/**
 * The id a new setup's module had in the previous run, following the
 * engine's remap; null when that id now names a different instance.
 */
export function previousModuleId(
    setup: EqPanelSetup,
    moduleId: string,
): string | null {
    for (const [from, to] of Object.entries(setup.moduleIdRemap)) {
        if (to === moduleId) {
            return from;
        }
    }
    return moduleId in setup.moduleIdRemap ? null : moduleId;
}
