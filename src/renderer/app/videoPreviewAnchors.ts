import { editor } from 'monaco-editor';
import { FIRST_LINE_COLUMN_OFFSET } from '../../shared/dsl/spanTypes';
import type { VideoPreviewSite } from '../../shared/video/videoGraph';
import type { VideoPreviewZone } from '../types/editor';
import { resolveScopeCallRange } from './scopeCallRange';

export interface VideoPreviewAnchors {
    zones: VideoPreviewZone[];
    /** Tracked ranges, one per resolvable zone; null when none resolved. */
    decorations: editor.IEditorDecorationsCollection | null;
}

/**
 * Anchors each `$v.preview` call with a tracked range so its panel follows the
 * call as the document is edited. A call whose line no longer exists (the
 * source changed during the async submit) gets a zone with no anchor, which
 * the editor hides.
 */
export function createVideoPreviewAnchors(
    editorInstance: editor.IStandaloneCodeEditor,
    sites: VideoPreviewSite[],
    callSiteSpans:
        | Record<string, { startLine: number; endLine: number }>
        | undefined,
    file: string,
): VideoPreviewAnchors {
    const model = editorInstance.getModel();
    const descriptions: editor.IModelDeltaDecoration[] = [];
    const zones = sites.map((site, index): VideoPreviewZone => {
        const loc = site.sourceLocation;
        const range =
            model && loc
                ? resolveScopeCallRange(
                      model,
                      {
                          column:
                              loc.line === 1
                                  ? loc.column - FIRST_LINE_COLUMN_OFFSET
                                  : loc.column,
                          line: loc.line,
                      },
                      callSiteSpans?.[`${loc.line}:${loc.column}`],
                  )
                : null;
        if (range) {
            descriptions.push({
                options: {
                    stickiness:
                        editor.TrackedRangeStickiness
                            .NeverGrowsWhenTypingAtEdges,
                },
                range,
            });
        }
        return {
            decorationIndex: range ? descriptions.length - 1 : null,
            file,
            index,
            view: site.view,
        };
    });
    return {
        decorations:
            descriptions.length > 0
                ? editorInstance.createDecorationsCollection(descriptions)
                : null,
        zones,
    };
}
