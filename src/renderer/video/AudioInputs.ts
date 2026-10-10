/// <reference types="@webgpu/types" />
import {
    VIDEO_HISTORY_LEN,
    type VideoHistory,
    type VideoTapSamples,
} from '../../shared/video/videoGraph';
import { alignWindow } from './alignWindow';
import { HistoryTexture } from './HistoryTexture';
import type { ShaderProgram } from './ShaderProgram';
import { TapStream } from './TapStream';

/**
 * The audio signals a shader reads, each played back against the display
 * clock, and the audio history texture that shows their recent samples.
 */
export class AudioInputs {
    readonly history: HistoryTexture;
    private readonly streams = new Map<number, TapStream>();
    /** Scratch the audio history windows are copied into, per history row. */
    private scratch: Float32Array[] = [];

    constructor(device: GPUDevice) {
        this.history = new HistoryTexture(device);
    }

    /**
     * Forgets every signal and sizes the history for `histories`. Taps are
     * numbered afresh by every compile, so what the streams hold belongs to
     * the previous patch's signals.
     */
    reset(histories: VideoHistory[]): void {
        this.streams.clear();
        this.history.resize(histories.length);
        this.scratch = histories.map(
            ({ samples, trigger }) =>
                new Float32Array(
                    trigger
                        ? Math.min(2 * samples, VIDEO_HISTORY_LEN)
                        : samples,
                ),
        );
    }

    /** Adds audio samples that have just arrived. */
    push(chunks: VideoTapSamples[]): void {
        const now = performance.now();
        for (const { tap, samples, sampleRate } of chunks) {
            let stream = this.streams.get(tap);
            if (stream === undefined) {
                stream = new TapStream();
                this.streams.set(tap, stream);
            }
            stream.push(samples, sampleRate, now);
        }
    }

    /** Whether every signal `program` reads has samples. */
    ready(program: ShaderProgram): boolean {
        const taps = [
            ...program.tapSlots.map(({ tap }) => tap),
            ...program.histories.map(({ tap }) => tap),
        ];
        return taps.every((tap) => this.streams.get(tap)?.hasData === true);
    }

    /**
     * Reads each signal at `nowMs`: its value into the uniform slots of
     * `program` it feeds, and its recent samples into the history rows that
     * show them.
     */
    update(program: ShaderProgram, nowMs: number): void {
        for (const stream of this.streams.values()) stream.advance(nowMs);
        for (const { slot, tap } of program.tapSlots) {
            program.setSlot(slot, this.streams.get(tap)?.value() ?? 0);
        }
        program.histories.forEach(({ tap, samples, trigger }, row) => {
            const stream = this.streams.get(tap);
            if (stream === undefined) return;
            const recent = this.scratch[row];
            stream.recent(recent);
            this.history.write(row, alignWindow(recent, samples, trigger));
        });
    }

    destroy(): void {
        this.history.destroy();
    }
}
