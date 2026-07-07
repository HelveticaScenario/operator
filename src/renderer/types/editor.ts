export type EditorBuffer =
    | {
          kind: 'file';
          id: string;
          filePath: string;
          content: string;
          dirty: boolean;
          isPreview?: boolean;
          /** App version this buffer last evaluated successfully under. Read
           *  from the file header on open, advanced on each successful evaluate,
           *  and written back to the header on save. Gates which migrations the
           *  patch still needs. Absent until the buffer first evaluates. */
          evaluatedVersion?: string;
      }
    | {
          kind: 'untitled';
          id: string;
          content: string;
          dirty: boolean;
          isPreview?: boolean;
          evaluatedVersion?: string;
      };

export type UnsavedBufferSnapshot =
    | {
          kind: 'file';
          id: string;
          filePath: string;
          content: string;
          evaluatedVersion?: string;
      }
    | {
          kind: 'untitled';
          id: string;
          content: string;
          evaluatedVersion?: string;
      };

export interface ScopeView {
    key: string;
    file: string;
    range: [number, number];
    channelKeys: string[];
}
