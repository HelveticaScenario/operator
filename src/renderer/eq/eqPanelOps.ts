/**
 * What the panel's section controls do. A plain change updates the running
 * module and writes the same quantized values into the code; a code-only
 * change writes the code and leaves a ghost. Adding or removing a section
 * changes the engine only once the code holds the change too, so code and
 * panel indices stay aligned.
 */
import { MAX_EQ_SECTIONS, quantizeGain, quantizeHz, quantizeQ } from './eqMath';
import {
    type EqField,
    type EqSectionChange,
    type EqSectionView,
    type RawEqSection,
    decodeSection,
    encodeChange,
    isPerChannel,
    sectionLists,
} from './eqPanelModel';
import {
    type CodeGhost,
    type EqPanelContext,
    FIELD_OF,
    type LoggedChange,
    type PanelRuntime,
    paramsAreModulated,
    parseSectionKey,
    quantizedLive,
    sectionKey,
} from './eqPanelRuntime';
import {
    applySourceOp,
    flushCode,
    scheduleCode,
    scheduleEngine,
    sendEngineNow,
} from './eqCodeWriter';
import type { EqListTarget } from './eqSourceLocate';
import type { EqChangePhase } from './eqGestures';

export function editable(ctx: EqPanelContext, panel: PanelRuntime): boolean {
    return panel.def.sourceLocation !== undefined && ctx.setup?.model != null;
}

/** The list being edited — the params' own array. */
function editedList(panel: PanelRuntime): RawEqSection[] {
    return sectionLists(panel.params)[panel.channel] ?? [];
}

/** The edited list's source shape, `count` sections long. */
function editedTarget(panel: PanelRuntime, count: number): EqListTarget {
    return {
        count,
        list: isPerChannel(panel.params) ? panel.channel : null,
        listCount: sectionLists(panel.params).length,
    };
}

function hasKeys(o: object): boolean {
    return Object.keys(o).length > 0;
}

/** Quantize to what the code will hold, dropping signal-driven fields. */
function quantizeChange(
    view: EqSectionView,
    change: EqSectionChange,
): EqSectionChange {
    const q: EqSectionChange = {};
    if (change.type !== undefined) {
        q.type = change.type;
    }
    if (change.freqHz !== undefined && !view.driven.freq) {
        q.freqHz = quantizeHz(change.freqHz);
    }
    if (change.gainDb !== undefined && !view.driven.gain) {
        q.gainDb = quantizeGain(change.gainDb);
    }
    if (change.q !== undefined && !view.driven.q) {
        q.q = quantizeQ(change.q);
    }
    return q;
}

/**
 * Apply a quantized change to section `index` of list `list`: into the
 * engine params for a plain change, into the ghost for a code-only one.
 */
function applyChange(
    panel: PanelRuntime,
    list: number,
    index: number,
    q: EqSectionChange,
    codeOnly: boolean,
): void {
    const raw = sectionLists(panel.params)[list]?.[index];
    if (!raw) {
        return;
    }
    const key = sectionKey(list, index);
    const view = decodeSection(raw);
    if (codeOnly) {
        const ghost: CodeGhost = { ...panel.ghosts.get(key), ...q };
        for (const k of Object.keys(ghost) as (keyof CodeGhost)[]) {
            if (ghost[k] === quantizedLive(view, k)) {
                delete ghost[k];
            }
        }
        if (hasKeys(ghost)) {
            panel.ghosts.set(key, ghost);
        } else {
            panel.ghosts.delete(key);
        }
        return;
    }
    const encoded = encodeChange(q, view.freqStyle);
    const ghost = panel.ghosts.get(key);
    for (const k of Object.keys(q) as (keyof EqSectionChange)[]) {
        const field = FIELD_OF[k];
        (raw as unknown as Record<string, unknown>)[field] =
            encoded[field]!.engine;
        if (ghost) {
            delete ghost[k];
        }
    }
    if (ghost && !hasKeys(ghost)) {
        panel.ghosts.delete(key);
    }
}

function logChange(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    list: number,
    index: number,
    q: EqSectionChange,
    codeOnly: boolean,
): void {
    const key = `${list}:${index}:${codeOnly}`;
    const prev = panel.changeLog.get(key);
    panel.changeLog.set(key, {
        change: { ...prev?.change, ...q },
        codeOnly,
        index,
        list,
        seq: ctx.nextEditSeq(),
    });
}

/** Adding or removing a section or list invalidates logged section
 *  indices. */
export function markStructure(ctx: EqPanelContext, panel: PanelRuntime): void {
    panel.structureSeq = ctx.nextEditSeq();
    panel.changeLog.clear();
    panel.modulated = paramsAreModulated(panel.params);
}

/** Queue `encoded` source literals for section `key`. */
function queueCode(
    panel: PanelRuntime,
    key: string,
    encoded: ReturnType<typeof encodeChange>,
): void {
    const pending = panel.pendingCode.get(key) ?? {};
    for (const field of Object.keys(encoded) as EqField[]) {
        pending[field] = encoded[field]!.code;
    }
    if (hasKeys(pending)) {
        panel.pendingCode.set(key, pending);
    }
}

export function changeSection(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    index: number,
    change: EqSectionChange,
    phase: EqChangePhase,
    codeOnly: boolean,
): void {
    const raw = editedList(panel)[index];
    if (!raw || (codeOnly && !editable(ctx, panel))) {
        return;
    }
    const view = decodeSection(raw);
    const q = quantizeChange(view, change);
    if (hasKeys(q)) {
        applyChange(panel, panel.channel, index, q, codeOnly);
        logChange(ctx, panel, panel.channel, index, q, codeOnly);
        if (!codeOnly) {
            scheduleEngine(ctx, panel);
        }
    }
    if (editable(ctx, panel)) {
        queueCode(
            panel,
            sectionKey(panel.channel, index),
            encodeChange(q, view.freqStyle),
        );
        if (phase === 'commit') {
            flushCode(ctx, panel, true);
        } else {
            scheduleCode(ctx, panel);
        }
    }
    ctx.render(panel);
}

/**
 * Write the audio's values back over a section's code-only edits (all of
 * them, or just `fields`), clearing their ghosts.
 */
export function revertSection(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    index: number,
    fields: (keyof EqSectionChange)[] | null,
): void {
    const key = sectionKey(panel.channel, index);
    const ghost = panel.ghosts.get(key);
    const raw = editedList(panel)[index];
    if (!ghost || !raw || !editable(ctx, panel)) {
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
    if (!hasKeys(ghost)) {
        panel.ghosts.delete(key);
    }
    logChange(ctx, panel, panel.channel, index, revert, true);
    queueCode(panel, key, encodeChange(revert, view.freqStyle));
    flushCode(ctx, panel, true);
    ctx.render(panel);
}

/**
 * Bring a panel committed from a queued run up to date with the changes
 * made after that run was submitted. Changes made after an add or remove
 * can't be matched to sections safely, so the panel only flags those.
 */
export function replayChanges(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    changes: LoggedChange[],
): void {
    let engine = false;
    for (const c of [...changes].sort((a, b) => a.seq - b.seq)) {
        applyChange(panel, c.list, c.index, c.change, c.codeOnly);
        engine ||= !c.codeOnly;
    }
    if (engine) {
        sendEngineNow(ctx, panel);
    }
}

export function addSection(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    freqHz: number,
    gainDb: number,
): void {
    const sections = editedList(panel);
    if (sections.length >= MAX_EQ_SECTIONS) {
        return;
    }
    flushCode(ctx, panel, false);
    const encoded = encodeChange(
        {
            freqHz: quantizeHz(freqHz),
            gainDb: quantizeGain(gainDb),
            type: 'peak',
        },
        'hz',
    );
    if (
        editable(ctx, panel) &&
        !applySourceOp(ctx, panel, editedTarget(panel, sections.length), {
            fields: {
                freq: encoded.freq!.code,
                gain: encoded.gain!.code,
                type: encoded.type!.code,
            },
            kind: 'add',
        })
    ) {
        ctx.render(panel);
        return;
    }
    sections.push({
        freq: encoded.freq!.engine,
        gain: encoded.gain!.engine,
        type: encoded.type!.engine as string,
    });
    panel.selected = sections.length - 1;
    markStructure(ctx, panel);
    sendEngineNow(ctx, panel);
    ctx.render(panel);
}

export function removeSection(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    index: number,
): void {
    const sections = editedList(panel);
    if (index < 0 || index >= sections.length) {
        return;
    }
    flushCode(ctx, panel, false);
    if (
        editable(ctx, panel) &&
        !applySourceOp(ctx, panel, editedTarget(panel, sections.length), {
            index,
            kind: 'remove',
        })
    ) {
        ctx.render(panel);
        return;
    }
    sections.splice(index, 1);
    const ghosts = new Map<string, CodeGhost>();
    for (const [key, g] of panel.ghosts) {
        const at = parseSectionKey(key);
        if (at.list !== panel.channel) {
            ghosts.set(key, g);
        } else if (at.index !== index) {
            ghosts.set(
                sectionKey(at.list, at.index > index ? at.index - 1 : at.index),
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
    markStructure(ctx, panel);
    sendEngineNow(ctx, panel);
    ctx.render(panel);
}
