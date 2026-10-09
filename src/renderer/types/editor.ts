import type { VideoPreviewView } from '../../shared/video/videoGraph';
export type EditorBuffer =
    | {
          kind: 'file';
          id: string;
          filePath: string;
          content: string;
          dirty: boolean;
          isPreview?: boolean;
      }
    | {
          kind: 'untitled';
          id: string;
          content: string;
          dirty: boolean;
          isPreview?: boolean;
      };

export type UnsavedBufferSnapshot =
    | {
          kind: 'file';
          id: string;
          filePath: string;
          content: string;
      }
    | {
          kind: 'untitled';
          id: string;
          content: string;
      };

export interface ScopeView {
    key: string;
    file: string;
    range: [number, number];
    channelKeys: string[];
    /**
     * Index of this view's tracked range in the scope decoration collection,
     * or null when the call site could not be resolved — the view then gets
     * no zone in the editor. Carried explicitly so views and decorations
     * never rely on sharing array positions.
     */
    decorationIndex: number | null;
}

/** One `$v.preview` panel in the editor. */
export interface VideoPreviewZone {
    /** Position among the shader's previews; frames carry the same index. */
    index: number;
    view: VideoPreviewView;
    file: string;
    /**
     * Index of this panel's tracked range in the preview decoration
     * collection, or null when the call site could not be resolved — the
     * panel then gets no zone.
     */
    decorationIndex: number | null;
}
