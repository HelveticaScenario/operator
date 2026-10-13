/**
 * Owns every inline EQ panel of the running patch: commits each patch run's
 * panels, routes the engine's live values and spectrum frames to them, and
 * renders them into their view zones. What the controls do lives in
 * `eqPanelOps`; how a change reaches the engine and the code lives in
 * `eqCodeWriter`.
 */
import React from 'react';
import type { editor } from 'monaco-editor';

import type { EqPanelDef } from '../../shared/dsl/eqPanelTypes';
import type { DSLExecuteResult } from '../../shared/ipcTypes';
import { scopeBufferKeyFromChannel } from '../app/oscilloscope';
import type { CodeStyle } from '../dsl/objectPropertyInsert';
import { EqLivePoller } from './eqLivePoller';
import { renderEqPanel } from './eqPanelRender';
import { cancelTimers, flushCode } from './eqCodeWriter';
import { type EqSectionView, isPerChannel, sectionLists } from './eqPanelModel';
import { addChannel, removeChannel, selectChannel } from './eqChannelOps';
import {
    addSection,
    changeSection,
    editable,
    removeSection,
    replayChanges,
    revertSection,
} from './eqPanelOps';
import {
    type EqPanelContext,
    type LiveValues,
    NOTICES,
    type PanelRuntime,
    mergeLive,
    paramsAreModulated,
    previousModuleId,
    shapeOf,
    shownLists,
} from './eqPanelRuntime';
import {
    type EqPanelSetup,
    createEqPanelSetup,
    disposeEqPanelSetup,
} from './eqPanelSetup';
import { SpectrumAnalyzer } from './eqSpectrum';
import { removeZone, sizeZones, syncZones } from './eqZoneHost';

export interface EqPanelControllerDeps {
    setModuleParam: (
        moduleId: string,
        moduleType: string,
        params: unknown,
    ) => Promise<unknown>;
    getSampleRate: () => Promise<number>;
    getModuleStates: () => Promise<Record<string, unknown>>;
}

type ScopeFrames = Map<
    string,
    { data: Float32Array; stats: { readOffset: number } }
>;

export class EqPanelController implements EqPanelContext {
    private readonly deps: EqPanelControllerDeps;
    private editorSubs: { dispose(): void }[] = [];
    private readonly panels = new Map<string, PanelRuntime>();
    private sampleRate = 48000;
    private readonly livePoller: EqLivePoller;
    private editSeq = 0;
    editor: editor.IStandaloneCodeEditor | null = null;
    setup: EqPanelSetup | null = null;
    codeStyle: CodeStyle | null = null;

    constructor(deps: EqPanelControllerDeps) {
        this.deps = deps;
        this.livePoller = new EqLivePoller(deps.getModuleStates, (states) => {
            for (const panel of this.panels.values()) {
                const live = mergeLive(
                    panel,
                    (
                        states[panel.def.moduleId] as
                            | { eqLive?: LiveValues }
                            | undefined
                    )?.eqLive ?? null,
                );
                if (live || panel.live) {
                    panel.live = live;
                    this.render(panel);
                }
            }
        });
    }

    setModuleParam(
        moduleId: string,
        moduleType: string,
        params: unknown,
    ): Promise<unknown> {
        return this.deps.setModuleParam(moduleId, moduleType, params);
    }

    nextEditSeq(): number {
        return ++this.editSeq;
    }

    /** Formatting for inserted code (the user's Prettier options). */
    setCodeStyle(style: CodeStyle): void {
        this.codeStyle = style;
    }

    attachEditor(next: editor.IStandaloneCodeEditor | null): void {
        if (next === this.editor) {
            return;
        }
        for (const panel of this.panels.values()) {
            removeZone(this.editor, panel);
        }
        for (const sub of this.editorSubs) {
            sub.dispose();
        }
        this.editorSubs = [];
        this.editor = next;
        if (next) {
            this.editorSubs = [
                next.onDidChangeModelContent(() => this.syncZones()),
                next.onDidChangeModel(() => this.syncZones()),
                next.onDidLayoutChange(() =>
                    sizeZones(next, this.panels.values()),
                ),
            ];
        }
        this.syncZones();
        this.updateLivePolling();
    }

    /** Build the panels of a submitted patch run, to commit when it applies. */
    stage(
        result: DSLExecuteResult,
        editorInstance: editor.IStandaloneCodeEditor | null,
    ): EqPanelSetup {
        return createEqPanelSetup(result, editorInstance, this.editSeq);
    }

    /** Make `setup` the live panel set (its patch run has applied). */
    commit(setup: EqPanelSetup): void {
        for (const panel of this.panels.values()) {
            flushCode(this, panel, true);
            cancelTimers(panel);
        }
        if (this.setup && this.setup !== setup) {
            disposeEqPanelSetup(this.setup);
        }
        this.setup = setup;

        const previous = new Map(
            [...this.panels.values()].map((p) => [p.def.moduleId, p]),
        );
        const next = new Map<string, PanelRuntime>();
        setup.defs.forEach((def, index) => {
            const params = setup.params.get(def.moduleId);
            if (!params) {
                return;
            }
            const prevId = previousModuleId(setup, def.moduleId);
            const prev = prevId === null ? undefined : previous.get(prevId);
            if (prev) {
                previous.delete(prevId!);
            }
            const panel = prev ?? this.newPanel(def);
            const lists = sectionLists(params);
            const channel = panel.channel < lists.length ? panel.channel : 0;
            const sameShape = shapeOf(panel.params) === shapeOf(params);
            const since = [...panel.changeLog.values()].filter(
                (c) => c.seq > setup.stagedSeq,
            );
            const structureSince = panel.structureSeq > setup.stagedSeq;
            Object.assign(panel, {
                channel,
                def,
                ghosts: new Map(),
                index,
                notice: null,
                params,
                selected:
                    panel.selected !== null &&
                    channel === panel.channel &&
                    panel.selected < lists[channel].length
                        ? panel.selected
                        : null,
                spectrumKeys: def.spectrum.channels.map((ch) =>
                    scopeBufferKeyFromChannel(
                        {
                            channel: ch,
                            moduleId: def.spectrum.moduleId,
                            portName: def.spectrum.portName,
                        },
                        def.spectrum.msPerFrame,
                    ),
                ),
            });
            panel.modulated = paramsAreModulated(params);
            panel.pendingCode.clear();
            if (structureSince || (since.length > 0 && !sameShape)) {
                panel.notice = NOTICES.queued;
            } else if (since.length > 0) {
                replayChanges(this, panel, since);
            }
            panel.changeLog.clear();
            next.set(def.key, panel);
        });
        for (const stale of previous.values()) {
            removeZone(this.editor, stale);
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
        this.updateLivePolling();
        this.renderAll();
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
            lists: shownLists(p),
            perChannel: isPerChannel(p.params),
            visible: p.zone !== null,
        }));
    }

    dispose(): void {
        for (const panel of this.panels.values()) {
            flushCode(this, panel, true);
            cancelTimers(panel);
        }
        this.livePoller.setActive(false);
        this.attachEditor(null);
        this.panels.clear();
        disposeEqPanelSetup(this.setup);
        this.setup = null;
    }

    private syncZones(): void {
        if (!this.editor) {
            return;
        }
        const added = syncZones(this.editor, this.setup, this.panels.values());
        if (added === null) {
            return;
        }
        for (const panel of added) {
            this.render(panel);
        }
        this.updateLivePolling();
    }

    /**
     * Poll the engine's live section values while a shown panel has a
     * signal-driven param, and only then.
     */
    private updateLivePolling(): void {
        this.livePoller.setActive(
            [...this.panels.values()].some(
                (p) => p.zone !== null && p.modulated,
            ),
        );
    }

    private newPanel(def: EqPanelDef): PanelRuntime {
        const frameListeners = new Set<() => void>();
        const panel: PanelRuntime = {
            callbacks: {
                onAdd: (freqHz, gainDb) =>
                    this.ifCurrent(panel, () =>
                        addSection(this, panel, freqHz, gainDb),
                    ),
                onAddChannel: () =>
                    this.ifCurrent(panel, () => addChannel(this, panel)),
                onChange: (index, change, phase, codeOnly) =>
                    this.ifCurrent(panel, () =>
                        changeSection(
                            this,
                            panel,
                            index,
                            change,
                            phase,
                            codeOnly,
                        ),
                    ),
                onChannel: (list) =>
                    this.ifCurrent(panel, () =>
                        selectChannel(this, panel, list),
                    ),
                onRemove: (index) =>
                    this.ifCurrent(panel, () =>
                        removeSection(this, panel, index),
                    ),
                onRemoveChannel: (list) =>
                    this.ifCurrent(panel, () =>
                        removeChannel(this, panel, list),
                    ),
                onRevert: (index, fields) =>
                    this.ifCurrent(panel, () =>
                        revertSection(this, panel, index, fields),
                    ),
                onSelect: (index) =>
                    this.ifCurrent(panel, () => {
                        panel.selected = index;
                        this.render(panel);
                    }),
                subscribeFrames: (cb) => {
                    frameListeners.add(cb);
                    return () => frameListeners.delete(cb);
                },
            },
            changeLog: new Map(),
            channel: 0,
            codeTimer: null,
            def,
            engineFrame: null,
            frameListeners,
            ghosts: new Map(),
            index: 0,
            lastCodeWrite: 0,
            live: null,
            modulated: false,
            notice: null,
            params: { input: 0, sections: [] },
            pendingCode: new Map(),
            selected: null,
            spectrum: new SpectrumAnalyzer(),
            spectrumKeys: [],
            structureSeq: 0,
            zone: null,
        };
        return panel;
    }

    /** Run a control's action only while its panel is still committed. */
    private ifCurrent(panel: PanelRuntime, action: () => void): void {
        if (this.panels.get(panel.def.key) === panel) {
            action();
        }
    }

    private renderAll(): void {
        for (const panel of this.panels.values()) {
            this.render(panel);
        }
    }

    render(panel: PanelRuntime): void {
        renderEqPanel(panel, {
            editable: editable(this, panel),
            sampleRate: this.sampleRate,
        });
    }
}
