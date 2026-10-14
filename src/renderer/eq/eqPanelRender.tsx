/** Renders a panel's current state into its view zone. */
import React from 'react';

import { EqPanel } from './EqPanel';
import { isPerChannel } from './eqPanelModel';
import { type PanelRuntime, sectionKey, shownLists } from './eqPanelRuntime';

/**
 * The edited list's sections (with live values for driven params) and, for
 * sections the code holds differently, the code-side view as a ghost.
 */
export function renderEqPanel(
    panel: PanelRuntime,
    opts: { editable: boolean; sampleRate: number },
): void {
    if (!panel.zone) {
        return;
    }
    const lists = shownLists(panel);
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
            editable={opts.editable}
            ghosts={ghosts}
            notice={panel.notice}
            sampleRate={opts.sampleRate}
            sections={sections}
            selected={panel.selected}
            spectrum={panel.spectrum}
            {...panel.callbacks}
        />,
    );
}
