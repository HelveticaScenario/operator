import type { VideoOutput } from './VideoOutput';
import type { VideoFeedbackConfig } from './VideoGraphBuilder';

/** The builder functions a buffer forwards to. */
interface BufferOps {
    readBuffer(index: number, config?: VideoFeedbackConfig): VideoOutput;
    writeBuffer(index: number, input: VideoOutput): VideoOutput;
}

/**
 * A frame store that persists from one frame to the next. Any signal can write
 * it and any number of signals can read what it held on the previous frame, so
 * buffers can feed themselves or each other.
 */
export class VideoBuffer {
    constructor(
        private readonly index: number,
        private readonly ops: BufferOps,
    ) {}

    /** The previous frame of this buffer, resampled through the transform in `config`. */
    read = (config?: VideoFeedbackConfig): VideoOutput =>
        this.ops.readBuffer(this.index, config);

    /** Stores `color` for the next frame to read, and returns it. */
    write = (color: VideoOutput): VideoOutput =>
        this.ops.writeBuffer(this.index, color);
}
