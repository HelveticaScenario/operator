import type { VideoPreviewFrame } from '../../shared/video/videoGraph';

const listeners = new Set<(frame: VideoPreviewFrame) => void>();

/** Hands a preview frame the video renderer drew to the editor's preview panels. */
export function publishPreviewFrame(frame: VideoPreviewFrame): void {
    for (const listener of listeners) listener(frame);
}

/** Calls `listener` with every preview frame; returns a function that stops it. */
export function subscribePreviewFrames(
    listener: (frame: VideoPreviewFrame) => void,
): () => void {
    listeners.add(listener);
    return () => {
        listeners.delete(listener);
    };
}
