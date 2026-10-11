/**
 * Owns every inline EQ panel of the running patch: its view zone and the
 * live params it edits. A plain gesture updates both the running module and
 * the call's literals; a Ctrl/Cmd gesture writes the code only and leaves a
 * ghost until the next patch run. Section adds and removes always apply to
 * both, so code and panel section indices stay aligned.
 */
import React from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { editor } from 'monaco-editor';

import { EQ_MODULE_TYPE, type EqPanelDef } from '../../shared/dsl/eqPanelTypes';
import { FIRST_LINE_COLUMN_OFFSET } from '../../shared/dsl/spanTypes';
import type { DSLExecuteResult } from '../../shared/ipcTypes';
import { scopeBufferKeyFromChannel } from '../app/oscilloscope';
import { resolveScopeCallRange } from '../app/scopeCallRange';
import type { CodeStyle } from '../dsl/objectPropertyInsert';
import { EQ_PANEL_HEIGHT, EqPanel, type EqChangePhase } from './EqPanel';
import {
    MAX_EQ_CHANNEL_LISTS,
    MAX_EQ_SECTIONS,
    quantizeGain,
    quantizeHz,
    quantizeQ,
} from './eqMath';
import {
    type EqField,
    type EqSectionChange,
    type EqSectionView,
    type RawEqParams,
    type RawEqSection,
    decodeSection,
    encodeChange,
    isPerChannel,
    type LiveSectionValues,
    sectionIsModulated,
    sectionLists,
    withLive,
} from './eqPanelModel';
import {
    type EqChannelOp,
    type EqListTarget,
    type EqSourceEditResult,
    type EqSourceOp,
    type FieldLiteral,
    computeEqChannelEdit,
    computeEqSourceEdit,
} from './eqSourceEdit';
import { SpectrumAnalyzer } from './eqSpectrum';

/** Interval between source writes while a control is moving. */
const CODE_WRITE_INTERVAL_MS = 150;
/** Poll interval for the live values of signal-driven section params. */
const LIVE_POLL_MS = 33;

const NOTICES = {
    skipped: 'non-literal values stay live only',
    stale: 'code changed by hand — run to resync',
    unsupported: 'sections are not an array literal — live only',
};

export interface EqPanelControllerDeps {
    setModuleParam: (
        moduleId: string,
        moduleType: string,
        params: unknown,
    ) => Promise<unknown>;
    getSampleRate: () => Promise<number>;
    getModuleStates: () => Promise<Record<string, unknown>>;
}

/**
 * Panels for one patch run, built when the run is submitted and committed
 * when the engine applies it (immediately, or at a queued bar/beat).
 */
export interface EqPanelSetup {
    defs: EqPanelDef[];
    params: Map<string, RawEqParams>;
    model: editor.ITextModel | null;
    /** One tracked range per def, from the callee to the call's last line. */
    decorations: editor.IEditorDecorationsCollection | null;
}

type ScopeFrames = Map<
    string,
    { data: Float32Array; stats: { readOffset: number } }
>;

/** Code-side values of a section that differ from the audio. */
type CodeGhost = EqSectionChange;

interface Zone {
    id: string;
    delegate: editor.IViewZone;
    root: Root;
}

interface PanelRuntime {
    def: EqPanelDef;
    /** Index into the setup's decorations. */
    index: number;
    params: RawEqParams;
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
    callbacks: PanelCallbacks;
}

interface PanelCallbacks {
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

function sectionKey(list: number, index: number): string {
    return `${list}:${index}`;
}

function parseSectionKey(key: string): { list: number; index: number } {
    const [list, index] = key.split(':').map(Number);
    return { index, list };
}

/** Live values per list and section; a section may not have reported yet. */
type LiveValues = (LiveSectionValues | undefined)[][];

const FIELD_OF: Record<keyof EqSectionChange, EqField> = {
    freqHz: 'freq',
    gainDb: 'gain',
    q: 'q',
    type: 'type',
};

/** A section's live value for `k`, quantized like a written value. */
function quantizedLive(
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
function mergeLive(
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

/** Build the panels' setup for a patch run, anchored in `model`. */
export function createEqPanelSetup(
    result: DSLExecuteResult,
    editorInstance: editor.IStandaloneCodeEditor | null,
): EqPanelSetup {
    const defs = result.eqPanels ?? [];
    const model = editorInstance?.getModel() ?? null;
    const params = new Map<string, RawEqParams>();
    for (const m of result.appliedPatch?.modules ?? []) {
        if (defs.some((d) => d.moduleId === m.id)) {
            params.set(m.id, structuredClone(m.params) as RawEqParams);
        }
    }
    if (!editorInstance || !model || defs.length === 0) {
        return { decorations: null, defs, model, params };
    }
    const descs: editor.IModelDeltaDecoration[] = defs.map((def) => {
        const loc = def.zoneLocation;
        const range = loc
            ? resolveScopeCallRange(
                  model,
                  {
                      column:
                          loc.line === 1
                              ? loc.column - FIRST_LINE_COLUMN_OFFSET
                              : loc.column,
                      line: loc.line,
                  },
                  result.callSiteSpans?.[`${loc.line}:${loc.column}`],
              )
            : null;
        return {
            options: {
                stickiness:
                    editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges,
            },
            range: range ?? {
                endColumn: 1,
                endLineNumber: 1,
                startColumn: 1,
                startLineNumber: 1,
            },
        };
    });
    return {
        decorations: editorInstance.createDecorationsCollection(descs),
        defs,
        model,
        params,
    };
}

export function disposeEqPanelSetup(setup: EqPanelSetup | null): void {
    setup?.decorations?.clear();
}

export class EqPanelController {
    private readonly deps: EqPanelControllerDeps;
    private editor: editor.IStandaloneCodeEditor | null = null;
    private editorSubs: { dispose(): void }[] = [];
    private setup: EqPanelSetup | null = null;
    private readonly panels = new Map<string, PanelRuntime>();
    private sampleRate = 48000;
    private liveTimer: number | null = null;
    private livePending = false;
    private codeStyle: CodeStyle | null = null;

    constructor(deps: EqPanelControllerDeps) {
        this.deps = deps;
    }

    /** Formatting for inserted code (the user's Prettier options). */
    setCodeStyle(style: CodeStyle): void {
        this.codeStyle = style;
    }

    attachEditor(next: editor.IStandaloneCodeEditor | null): void {
        if (next === this.editor) {
            return;
        }
        this.removeAllZones();
        this.updateLivePolling();
        for (const sub of this.editorSubs) {
            sub.dispose();
        }
        this.editorSubs = [];
        this.editor = next;
        if (next) {
            this.editorSubs = [
                next.onDidChangeModelContent(() => this.syncZones()),
                next.onDidChangeModel(() => this.syncZones()),
                next.onDidLayoutChange(() => this.sizeZones()),
            ];
        }
        this.syncZones();
    }

    /** Make `setup` the live panel set (its patch run has applied). */
    commit(setup: EqPanelSetup): void {
        for (const panel of this.panels.values()) {
            this.flushCode(panel, true);
            this.cancelTimers(panel);
        }
        if (this.setup && this.setup !== setup) {
            disposeEqPanelSetup(this.setup);
        }
        this.setup = setup;

        const next = new Map<string, PanelRuntime>();
        setup.defs.forEach((def, index) => {
            const params = setup.params.get(def.moduleId);
            if (!params) {
                return;
            }
            if (!Array.isArray(params.sections)) {
                params.sections = [];
            }
            const prev = this.panels.get(def.key);
            this.panels.delete(def.key);
            const spectrumKeys = def.spectrum.channels.map((channel) =>
                scopeBufferKeyFromChannel(
                    {
                        channel,
                        moduleId: def.spectrum.moduleId,
                        portName: def.spectrum.portName,
                    },
                    def.spectrum.msPerFrame,
                ),
            );
            const lists = sectionLists(params);
            const channel =
                prev && prev.channel < lists.length ? prev.channel : 0;
            const selected =
                prev?.selected != null &&
                prev.channel === channel &&
                prev.selected < lists[channel].length
                    ? prev.selected
                    : null;
            const panel: PanelRuntime = prev ?? this.newPanel(def);
            Object.assign(panel, {
                channel,
                def,
                ghosts: new Map(),
                index,
                notice: null,
                params,
                selected,
                spectrumKeys,
            });
            panel.pendingCode.clear();
            next.set(def.key, panel);
        });
        for (const stale of this.panels.values()) {
            this.removeZone(stale);
        }
        this.panels.clear();
        for (const [key, panel] of next) {
            this.panels.set(key, panel);
        }

        void this.deps
            .getSampleRate()
            .then((rate) => {
                if (rate > 0 && rate !== this.sampleRate) {
                    this.sampleRate = rate;
                    this.renderAll();
                }
            })
            .catch(() => {});
        this.syncZones();
        this.renderAll();
    }

    /**
     * Poll the engine's live section values while a shown panel has a
     * signal-driven param, and only then.
     */
    private updateLivePolling(): void {
        const needed = [...this.panels.values()].some(
            (p) =>
                p.zone !== null &&
                sectionLists(p.params).some((l) =>
                    l.some((raw) => sectionIsModulated(decodeSection(raw))),
                ),
        );
        if (needed && this.liveTimer === null) {
            this.liveTimer = window.setInterval(() => {
                if (this.livePending) {
                    return;
                }
                this.livePending = true;
                void this.deps
                    .getModuleStates()
                    .then((states) => {
                        for (const panel of this.panels.values()) {
                            const state = states[panel.def.moduleId] as
                                | { eqLive?: LiveValues }
                                | undefined;
                            const live = mergeLive(
                                panel,
                                state?.eqLive ?? null,
                            );
                            if (live || panel.live) {
                                panel.live = live;
                                this.render(panel);
                            }
                        }
                    })
                    .catch(() => {})
                    .finally(() => {
                        this.livePending = false;
                    });
            }, LIVE_POLL_MS);
        } else if (!needed && this.liveTimer !== null) {
            window.clearInterval(this.liveTimer);
            this.liveTimer = null;
        }
    }

    /** Every list's sections as shown: params plus live driven values. */
    private shownLists(panel: PanelRuntime): EqSectionView[][] {
        return sectionLists(panel.params).map((list, l) =>
            list.map((raw, i) =>
                withLive(decodeSection(raw), panel.live?.[l]?.[i]),
            ),
        );
    }

    /** Feed one scope poll to the panels' spectra. */
    onScopeFrames(frames: ScopeFrames, nowMs: number): void {
        for (const panel of this.panels.values()) {
            if (!panel.zone) {
                continue;
            }
            const entry = frames.get(panel.spectrumKeys[panel.channel] ?? '');
            if (!entry) {
                continue;
            }
            panel.spectrum.sampleRate = this.sampleRate;
            if (
                panel.spectrum.push(entry.data, entry.stats.readOffset, nowMs)
            ) {
                panel.spectrum.compute(nowMs);
                for (const cb of panel.frameListeners) {
                    cb();
                }
            }
        }
    }

    /** Audio stopped: clear the spectra. */
    onStopped(): void {
        for (const panel of this.panels.values()) {
            panel.spectrum.reset();
            for (const cb of panel.frameListeners) {
                cb();
            }
        }
    }

    /** Live panel state, for tests. */
    snapshot(): {
        key: string;
        lists: EqSectionView[][];
        perChannel: boolean;
        visible: boolean;
    }[] {
        return [...this.panels.values()].map((p) => ({
            key: p.def.key,
            lists: this.shownLists(p),
            perChannel: isPerChannel(p.params),
            visible: p.zone !== null,
        }));
    }

    dispose(): void {
        for (const panel of this.panels.values()) {
            this.flushCode(panel, true);
            this.cancelTimers(panel);
        }
        if (this.liveTimer !== null) {
            window.clearInterval(this.liveTimer);
            this.liveTimer = null;
        }
        this.attachEditor(null);
        this.panels.clear();
        disposeEqPanelSetup(this.setup);
        this.setup = null;
    }

    private newPanel(def: EqPanelDef): PanelRuntime {
        const key = def.key;
        const withPanel = (fn: (panel: PanelRuntime) => void) => {
            const panel = this.panels.get(key);
            if (panel) {
                fn(panel);
            }
        };
        const frameListeners = new Set<() => void>();
        return {
            callbacks: {
                onAdd: (freqHz, gainDb) =>
                    withPanel((p) => this.addSection(p, freqHz, gainDb)),
                onChange: (index, change, phase, codeOnly) =>
                    withPanel((p) =>
                        this.changeSection(p, index, change, phase, codeOnly),
                    ),
                onChannel: (list) =>
                    withPanel((p) => this.selectChannel(p, list)),
                onAddChannel: () => withPanel((p) => this.addChannel(p)),
                onRemove: (index) =>
                    withPanel((p) => this.removeSection(p, index)),
                onRemoveChannel: (list) =>
                    withPanel((p) => this.removeChannel(p, list)),
                onRevert: (index, fields) =>
                    withPanel((p) => this.revertSection(p, index, fields)),
                onSelect: (index) =>
                    withPanel((p) => {
                        p.selected = index;
                        this.render(p);
                    }),
                subscribeFrames: (cb) => {
                    frameListeners.add(cb);
                    return () => frameListeners.delete(cb);
                },
            },
            channel: 0,
            codeTimer: null,
            def,
            engineFrame: null,
            frameListeners,
            ghosts: new Map(),
            index: 0,
            lastCodeWrite: 0,
            notice: null,
            params: { input: 0, sections: [] },
            pendingCode: new Map(),
            selected: null,
            live: null,
            spectrum: new SpectrumAnalyzer(),
            spectrumKeys: [],
            zone: null,
        };
    }

    /** The list being edited — the params' own array. */
    private list(panel: PanelRuntime): RawEqSection[] {
        return sectionLists(panel.params)[panel.channel] ?? [];
    }

    /** The edited list's source shape, `count` sections long. */
    private target(panel: PanelRuntime, count: number): EqListTarget {
        const perChannel = isPerChannel(panel.params);
        return {
            count,
            list: perChannel ? panel.channel : null,
            listCount: sectionLists(panel.params).length,
        };
    }

    private selectChannel(panel: PanelRuntime, list: number): void {
        if (
            list === panel.channel ||
            list >= sectionLists(panel.params).length
        ) {
            return;
        }
        this.flushCode(panel, true);
        panel.channel = list;
        panel.selected = null;
        panel.spectrum.reset();
        this.render(panel);
    }

    private editable(panel: PanelRuntime): boolean {
        return (
            panel.def.sourceLocation !== undefined && this.setup?.model != null
        );
    }

    private changeSection(
        panel: PanelRuntime,
        index: number,
        change: EqSectionChange,
        phase: EqChangePhase,
        codeOnly: boolean,
    ): void {
        const raw = this.list(panel)[index];
        if (!raw) {
            return;
        }
        const key = sectionKey(panel.channel, index);
        if (codeOnly && !this.editable(panel)) {
            return;
        }
        const view = decodeSection(raw);
        // Quantize to what the code will hold; drop signal-driven fields.
        const q: EqSectionChange = {};
        if (change.type !== undefined) {
            q.type = change.type;
        }
        if (change.freqHz !== undefined && view.freqHz !== null) {
            q.freqHz = quantizeHz(change.freqHz);
        }
        if (change.gainDb !== undefined && view.gainDb !== null) {
            q.gainDb = quantizeGain(change.gainDb);
        }
        if (change.q !== undefined && view.q !== null) {
            q.q = quantizeQ(change.q);
        }
        const encoded = encodeChange(q, view.freqStyle);

        if (codeOnly) {
            const ghost: CodeGhost = { ...panel.ghosts.get(key), ...q };
            for (const k of Object.keys(ghost) as (keyof CodeGhost)[]) {
                if (ghost[k] === quantizedLive(view, k)) {
                    delete ghost[k];
                }
            }
            if (Object.keys(ghost).length > 0) {
                panel.ghosts.set(key, ghost);
            } else {
                panel.ghosts.delete(key);
            }
        } else {
            const ghost = panel.ghosts.get(key);
            for (const k of Object.keys(q) as (keyof EqSectionChange)[]) {
                const field = FIELD_OF[k];
                (raw as unknown as Record<string, unknown>)[field] =
                    encoded[field]!.engine;
                if (ghost) {
                    delete ghost[k];
                }
            }
            if (ghost && Object.keys(ghost).length === 0) {
                panel.ghosts.delete(key);
            }
            if (Object.keys(q).length > 0) {
                this.scheduleEngine(panel);
            }
        }

        if (this.editable(panel)) {
            const pending = panel.pendingCode.get(key) ?? {};
            for (const field of Object.keys(encoded) as EqField[]) {
                pending[field] = encoded[field]!.code;
            }
            if (Object.keys(pending).length > 0) {
                panel.pendingCode.set(key, pending);
            }
            if (phase === 'commit') {
                this.flushCode(panel, true);
            } else {
                this.scheduleCode(panel);
            }
        }
        this.render(panel);
    }

    private addSection(
        panel: PanelRuntime,
        freqHz: number,
        gainDb: number,
    ): void {
        const sections = this.list(panel);
        if (sections.length >= MAX_EQ_SECTIONS) {
            return;
        }
        this.flushCode(panel, false);
        const change: EqSectionChange = {
            freqHz: quantizeHz(freqHz),
            gainDb: quantizeGain(gainDb),
            type: 'peak',
        };
        const encoded = encodeChange(change, 'hz');
        if (this.editable(panel)) {
            this.applySourceOp(panel, this.target(panel, sections.length), {
                fields: {
                    freq: encoded.freq!.code,
                    gain: encoded.gain!.code,
                    type: encoded.type!.code,
                },
                kind: 'add',
            });
        }
        const section: RawEqSection = {
            freq: encoded.freq!.engine,
            gain: encoded.gain!.engine,
            type: encoded.type!.engine as string,
        };
        sections.push(section);
        panel.selected = sections.length - 1;
        this.sendEngineNow(panel);
        this.render(panel);
    }

    /**
     * Write the audio's values back over a section's code-only edits (all of
     * them, or just `fields`), clearing their ghosts.
     */
    private revertSection(
        panel: PanelRuntime,
        index: number,
        fields: (keyof EqSectionChange)[] | null,
    ): void {
        const key = sectionKey(panel.channel, index);
        const ghost = panel.ghosts.get(key);
        const raw = this.list(panel)[index];
        if (!ghost || !raw || !this.editable(panel)) {
            return;
        }
        const view = decodeSection(raw);
        const revert: EqSectionChange = {};
        for (const k of Object.keys(ghost) as (keyof EqSectionChange)[]) {
            if (fields && !fields.includes(k)) {
                continue;
            }
            delete ghost[k];
            if (k === 'type') {
                revert.type = view.type;
                continue;
            }
            const live = quantizedLive(view, k) as number | null;
            if (live !== null) {
                revert[k] = live;
            }
        }
        if (Object.keys(ghost).length === 0) {
            panel.ghosts.delete(key);
        }
        const encoded = encodeChange(revert, view.freqStyle);
        const pending = panel.pendingCode.get(key) ?? {};
        for (const field of Object.keys(encoded) as EqField[]) {
            pending[field] = encoded[field]!.code;
        }
        panel.pendingCode.set(key, pending);
        this.flushCode(panel, true);
        this.render(panel);
    }

    /**
     * Add a channel list copying the edited one; the shared form splits into
     * two copies. Code-only values travel with the copy, since the code copy
     * carries them.
     */
    private addChannel(panel: PanelRuntime): void {
        const lists = sectionLists(panel.params);
        const perChannel = isPerChannel(panel.params);
        if (lists.length >= MAX_EQ_CHANNEL_LISTS) {
            return;
        }
        this.flushCode(panel, false);
        const from = perChannel ? panel.channel : 0;
        if (this.editable(panel)) {
            this.applyChannelOp(
                panel,
                perChannel ? lists.length : 0,
                perChannel ? { from, kind: 'addChannel' } : { kind: 'split' },
            );
        }
        const copy = structuredClone(lists[from]);
        panel.params.sections = perChannel
            ? [...lists, copy]
            : [lists[0], copy];
        const added = sectionLists(panel.params).length - 1;
        for (const [key, g] of [...panel.ghosts]) {
            const at = parseSectionKey(key);
            if (at.list === from) {
                panel.ghosts.set(sectionKey(added, at.index), { ...g });
            }
        }
        panel.live = null;
        this.selectChannel(panel, added);
        this.sendEngineNow(panel);
        this.render(panel);
    }

    /** Remove channel list `list`; a single remaining list becomes shared. */
    private removeChannel(panel: PanelRuntime, list: number): void {
        const lists = sectionLists(panel.params);
        if (!isPerChannel(panel.params) || list < 0 || list >= lists.length) {
            return;
        }
        this.flushCode(panel, false);
        if (this.editable(panel)) {
            this.applyChannelOp(panel, lists.length, {
                index: list,
                kind: 'removeChannel',
            });
        }
        const rest = lists.filter((_, l) => l !== list);
        panel.params.sections = rest.length === 1 ? rest[0] : rest;
        const ghosts = new Map<string, CodeGhost>();
        for (const [key, g] of panel.ghosts) {
            const at = parseSectionKey(key);
            if (at.list !== list) {
                ghosts.set(
                    sectionKey(
                        at.list > list ? at.list - 1 : at.list,
                        at.index,
                    ),
                    g,
                );
            }
        }
        panel.ghosts = ghosts;
        panel.live = null;
        panel.channel = Math.min(
            panel.channel > list ? panel.channel - 1 : panel.channel,
            rest.length - 1,
        );
        panel.selected = null;
        panel.spectrum.reset();
        this.sendEngineNow(panel);
        this.render(panel);
    }

    private applyChannelOp(
        panel: PanelRuntime,
        listCount: number,
        op: EqChannelOp,
    ): void {
        this.applyEdit(panel, (source, anchor, layout) =>
            computeEqChannelEdit(source, anchor, listCount, op, layout),
        );
    }

    private removeSection(panel: PanelRuntime, index: number): void {
        const sections = this.list(panel);
        if (index < 0 || index >= sections.length) {
            return;
        }
        this.flushCode(panel, false);
        if (this.editable(panel)) {
            this.applySourceOp(panel, this.target(panel, sections.length), {
                index,
                kind: 'remove',
            });
        }
        sections.splice(index, 1);
        const ghosts = new Map<string, CodeGhost>();
        for (const [key, g] of panel.ghosts) {
            const at = parseSectionKey(key);
            if (at.list !== panel.channel) {
                ghosts.set(key, g);
            } else if (at.index !== index) {
                ghosts.set(
                    sectionKey(
                        at.list,
                        at.index > index ? at.index - 1 : at.index,
                    ),
                    g,
                );
            }
        }
        panel.ghosts = ghosts;
        if (panel.selected !== null) {
            panel.selected =
                panel.selected === index
                    ? null
                    : panel.selected > index
                      ? panel.selected - 1
                      : panel.selected;
        }
        this.sendEngineNow(panel);
        this.render(panel);
    }

    private scheduleEngine(panel: PanelRuntime): void {
        if (panel.engineFrame !== null) {
            return;
        }
        panel.engineFrame = requestAnimationFrame(() => {
            panel.engineFrame = null;
            this.sendEngine(panel);
        });
    }

    private sendEngineNow(panel: PanelRuntime): void {
        if (panel.engineFrame !== null) {
            cancelAnimationFrame(panel.engineFrame);
            panel.engineFrame = null;
        }
        this.sendEngine(panel);
    }

    private sendEngine(panel: PanelRuntime): void {
        this.deps
            .setModuleParam(panel.def.moduleId, EQ_MODULE_TYPE, panel.params)
            .catch((err) => {
                console.error('EQ panel update failed:', err);
            });
    }

    private scheduleCode(panel: PanelRuntime): void {
        if (panel.codeTimer !== null) {
            return;
        }
        const elapsed = performance.now() - panel.lastCodeWrite;
        if (elapsed >= CODE_WRITE_INTERVAL_MS) {
            this.flushCode(panel, false);
            return;
        }
        panel.codeTimer = window.setTimeout(() => {
            panel.codeTimer = null;
            this.flushCode(panel, false);
        }, CODE_WRITE_INTERVAL_MS - elapsed);
    }

    /** Write pending literals now; `endGesture` closes the undo step. */
    private flushCode(panel: PanelRuntime, endGesture: boolean): void {
        if (panel.codeTimer !== null) {
            window.clearTimeout(panel.codeTimer);
            panel.codeTimer = null;
        }
        if (panel.pendingCode.size > 0) {
            panel.lastCodeWrite = performance.now();
            const pending = [...panel.pendingCode];
            panel.pendingCode.clear();
            const lists = sectionLists(panel.params);
            for (const [key, fields] of pending) {
                const { list, index } = parseSectionKey(key);
                const perChannel = isPerChannel(panel.params);
                this.applySourceOp(
                    panel,
                    {
                        count: lists[list]?.length ?? 0,
                        list: perChannel ? list : null,
                        listCount: lists.length,
                    },
                    { fields, index, kind: 'set' },
                    false,
                );
            }
        }
        if (endGesture) {
            this.setup?.model?.pushStackElement();
        }
    }

    /** Apply one source op at the panel's anchor, recording any notice. */
    private applySourceOp(
        panel: PanelRuntime,
        target: EqListTarget,
        op: EqSourceOp,
        closeUndo = true,
    ): void {
        this.applyEdit(
            panel,
            (source, anchor, layout) =>
                computeEqSourceEdit(source, anchor, target, op, layout),
            closeUndo,
        );
    }

    /** Compute an edit at the panel's anchor and apply it, recording any
     *  notice. */
    private applyEdit(
        panel: PanelRuntime,
        compute: (
            source: string,
            anchor: number,
            layout: CodeStyle,
        ) => EqSourceEditResult,
        closeUndo = true,
    ): void {
        const model = this.setup?.model;
        const range = this.setup?.decorations?.getRange(panel.index);
        if (
            !this.codeStyle ||
            !model ||
            model.isDisposed() ||
            !range ||
            range.isEmpty()
        ) {
            return;
        }
        const anchor = model.getOffsetAt(range.getStartPosition());
        const result = compute(model.getValue(), anchor, this.codeStyle);
        if (!result.ok) {
            panel.notice = NOTICES[result.reason];
            return;
        }
        if (result.skipped.length > 0) {
            panel.notice = NOTICES.skipped;
        }
        if (!result.edit) {
            return;
        }
        const start = model.getPositionAt(result.edit.start);
        const end = model.getPositionAt(result.edit.end);
        const visible = this.editor?.getModel() === model;
        const selections = visible ? (this.editor!.getSelections() ?? []) : [];
        if (closeUndo) {
            model.pushStackElement();
        }
        model.pushEditOperations(
            selections,
            [
                {
                    range: {
                        endColumn: end.column,
                        endLineNumber: end.lineNumber,
                        startColumn: start.column,
                        startLineNumber: start.lineNumber,
                    },
                    text: result.edit.text,
                },
            ],
            () => (visible ? selections : null),
        );
        if (closeUndo) {
            model.pushStackElement();
        }
    }

    /** Drop scheduled work; a pending engine send is superseded by the
     *  patch being committed. */
    private cancelTimers(panel: PanelRuntime): void {
        if (panel.codeTimer !== null) {
            window.clearTimeout(panel.codeTimer);
            panel.codeTimer = null;
        }
        if (panel.engineFrame !== null) {
            cancelAnimationFrame(panel.engineFrame);
            panel.engineFrame = null;
        }
    }

    /** The after-line for a panel's zone, or null when it has no anchor. */
    private zoneLine(panel: PanelRuntime): number | null {
        const setup = this.setup;
        if (
            !setup?.model ||
            setup.model.isDisposed() ||
            this.editor?.getModel() !== setup.model
        ) {
            return null;
        }
        const range = setup.decorations?.getRange(panel.index);
        return range && !range.isEmpty() ? range.endLineNumber : null;
    }

    /** Add, move, or remove zones to match the anchors. */
    private syncZones(): void {
        const ed = this.editor;
        if (!ed) {
            return;
        }
        const added: PanelRuntime[] = [];
        ed.changeViewZones((accessor) => {
            for (const panel of this.panels.values()) {
                const line = this.zoneLine(panel);
                if (line === null) {
                    if (panel.zone) {
                        accessor.removeZone(panel.zone.id);
                        this.unmountZone(panel.zone);
                        panel.zone = null;
                    }
                    continue;
                }
                if (panel.zone) {
                    if (panel.zone.delegate.afterLineNumber !== line) {
                        panel.zone.delegate.afterLineNumber = line;
                        accessor.layoutZone(panel.zone.id);
                    }
                    continue;
                }
                const dom = document.createElement('div');
                dom.className = 'eq-view-zone';
                // Above Monaco's .view-lines layer, or clicks move the cursor.
                Object.assign(dom.style, {
                    pointerEvents: 'auto',
                    position: 'relative',
                    zIndex: '10',
                });
                for (const type of ['mousedown', 'pointerdown']) {
                    dom.addEventListener(type, (e) => e.stopPropagation());
                }
                // The editor's context menu never opens over the panel; the
                // panel's own right-click gestures are React handlers on this
                // same element, so they still run.
                dom.addEventListener('contextmenu', (e) => {
                    e.preventDefault();
                    e.stopPropagation();
                });
                const delegate: editor.IViewZone = {
                    afterLineNumber: line,
                    domNode: dom,
                    heightInPx: EQ_PANEL_HEIGHT,
                    suppressMouseDown: true,
                };
                panel.zone = {
                    delegate,
                    id: accessor.addZone(delegate),
                    root: createRoot(dom),
                };
                added.push(panel);
            }
        });
        if (added.length > 0) {
            this.sizeZones();
        }
        for (const panel of added) {
            this.render(panel);
        }
        this.updateLivePolling();
    }

    /**
     * Pin each zone to the visible text area: a view zone otherwise spans the
     * widest line, pushing the panel's right side out of view.
     */
    private sizeZones(): void {
        const layout = this.editor?.getLayoutInfo();
        if (!layout) {
            return;
        }
        const width = Math.max(
            0,
            layout.width - layout.contentLeft - layout.verticalScrollbarWidth,
        );
        for (const panel of this.panels.values()) {
            if (panel.zone) {
                panel.zone.delegate.domNode.style.width = `${width}px`;
            }
        }
    }

    private removeZone(panel: PanelRuntime): void {
        if (!panel.zone) {
            return;
        }
        const zone = panel.zone;
        panel.zone = null;
        try {
            this.editor?.changeViewZones((accessor) =>
                accessor.removeZone(zone.id),
            );
        } catch {
            // The editor is already disposed, taking its zones with it.
        }
        this.unmountZone(zone);
    }

    private removeAllZones(): void {
        for (const panel of this.panels.values()) {
            this.removeZone(panel);
        }
    }

    private unmountZone(zone: Zone): void {
        // Unmounting synchronously inside a React commit warns; defer it.
        queueMicrotask(() => zone.root.unmount());
    }

    private renderAll(): void {
        for (const panel of this.panels.values()) {
            this.render(panel);
        }
    }

    private render(panel: PanelRuntime): void {
        if (!panel.zone) {
            return;
        }
        const lists = this.shownLists(panel);
        const sections = lists[panel.channel] ?? [];
        const ghosts = sections.map((s, i) => {
            const g = panel.ghosts.get(sectionKey(panel.channel, i));
            return g
                ? {
                      ...s,
                      ...(g.type !== undefined && { type: g.type }),
                      ...(g.freqHz !== undefined && { freqHz: g.freqHz }),
                      ...(g.gainDb !== undefined && { gainDb: g.gainDb }),
                      ...(g.q !== undefined && { q: g.q }),
                  }
                : null;
        });
        panel.zone.root.render(
            <EqPanel
                adaptiveQ={panel.params.adaptiveQ === true}
                channel={panel.channel}
                channelLists={isPerChannel(panel.params) ? lists : null}
                editable={this.editable(panel)}
                ghosts={ghosts}
                notice={panel.notice}
                sampleRate={this.sampleRate}
                sections={sections}
                selected={panel.selected}
                spectrum={panel.spectrum}
                {...panel.callbacks}
            />,
        );
    }
}
