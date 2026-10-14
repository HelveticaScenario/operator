/**
 * The panel's channel-list controls: switch the edited list, add a copy of
 * it (the shared form splits into two), or remove one (a single remaining
 * list becomes the shared form). Like section adds and removes, they change
 * the engine only once the code holds the change.
 */
import { computeEqChannelEdit, type EqChannelOp } from './eqChannelEdit';
import { applyEdit, flushCode, sendEngineNow } from './eqCodeWriter';
import { MAX_EQ_CHANNEL_LISTS } from './eqMath';
import { isPerChannel, sectionLists } from './eqPanelModel';
import { editable, markStructure } from './eqPanelOps';
import {
    type CodeGhost,
    type EqPanelContext,
    type PanelRuntime,
    parseSectionKey,
    sectionKey,
} from './eqPanelRuntime';

export function selectChannel(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    list: number,
): void {
    if (list === panel.channel || list >= sectionLists(panel.params).length) {
        return;
    }
    flushCode(ctx, panel, true);
    panel.channel = list;
    panel.selected = null;
    panel.spectrum.reset();
    ctx.render(panel);
}

function applyChannelOp(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    listCount: number,
    op: EqChannelOp,
): boolean {
    return applyEdit(ctx, panel, (source, anchor, layout) =>
        computeEqChannelEdit(source, anchor, listCount, op, layout),
    );
}

/**
 * Add a channel list copying the edited one; the shared form splits into
 * two copies. Code-only values travel with the copy, since the code copy
 * carries them.
 */
export function addChannel(ctx: EqPanelContext, panel: PanelRuntime): void {
    const lists = sectionLists(panel.params);
    const perChannel = isPerChannel(panel.params);
    if (lists.length >= MAX_EQ_CHANNEL_LISTS) {
        return;
    }
    flushCode(ctx, panel, false);
    const from = perChannel ? panel.channel : 0;
    if (
        editable(ctx, panel) &&
        !applyChannelOp(
            ctx,
            panel,
            perChannel ? lists.length : 0,
            perChannel ? { from, kind: 'addChannel' } : { kind: 'split' },
        )
    ) {
        ctx.render(panel);
        return;
    }
    const copy = structuredClone(lists[from]);
    panel.params.sections = perChannel ? [...lists, copy] : [lists[0], copy];
    const added = sectionLists(panel.params).length - 1;
    for (const [key, g] of [...panel.ghosts]) {
        const at = parseSectionKey(key);
        if (at.list === from) {
            panel.ghosts.set(sectionKey(added, at.index), { ...g });
        }
    }
    panel.live = null;
    markStructure(ctx, panel);
    selectChannel(ctx, panel, added);
    sendEngineNow(ctx, panel);
    ctx.render(panel);
}

/** Remove channel list `list`; a single remaining list becomes shared. */
export function removeChannel(
    ctx: EqPanelContext,
    panel: PanelRuntime,
    list: number,
): void {
    const lists = sectionLists(panel.params);
    if (!isPerChannel(panel.params) || list < 0 || list >= lists.length) {
        return;
    }
    flushCode(ctx, panel, false);
    if (
        editable(ctx, panel) &&
        !applyChannelOp(ctx, panel, lists.length, {
            index: list,
            kind: 'removeChannel',
        })
    ) {
        ctx.render(panel);
        return;
    }
    const rest = lists.filter((_, l) => l !== list);
    panel.params.sections = rest.length === 1 ? rest[0] : rest;
    const ghosts = new Map<string, CodeGhost>();
    for (const [key, g] of panel.ghosts) {
        const at = parseSectionKey(key);
        if (at.list !== list) {
            ghosts.set(
                sectionKey(at.list > list ? at.list - 1 : at.list, at.index),
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
    markStructure(ctx, panel);
    sendEngineNow(ctx, panel);
    ctx.render(panel);
}
