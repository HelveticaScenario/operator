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
    /** Scratch the recent samples are copied into, per history row. */
    private scratch: Float32Array[] = [];
    /** The window each history row shows, rewritten every frame. */
    private windows: Float32Array<ArrayBuffer>[] = [];
    private histories: VideoHistory[] = [];
    /** Each tap slot's signal stamp in the shader the streams belong to. */
    private stamps: readonly number[] = [];

    constructor(device: GPUDevice) {
        this.history = new HistoryTexture(device);
    }

    /**
     * Sizes the history for `histories` and forgets every signal whose tap
     * slot carries another signal now, which `stamps` tells: a slot keeps its
     * samples when its stamp is the same as before. Returns the slots
     * forgotten, which must be read afresh.
     */
    reset(histories: VideoHistory[], stamps: readonly number[] = []): number[] {
        const forgotten: number[] = [];
        for (const tap of this.streams.keys()) {
            const stamp = stamps[tap] ?? 0;
            if (stamp === 0 || stamp !== this.stamps[tap]) {
                this.streams.delete(tap);
                forgotten.push(tap);
            }
        }
        stamps.forEach((stamp, tap) => {
            if (
                stamp !== 0 &&
                !this.streams.has(tap) &&
                !forgotten.includes(tap)
            ) {
                forgotten.push(tap);
            }
        });
        this.stamps = stamps;
        this.histories = histories;
        this.history.resize(histories.length);
        this.scratch = histories.map(
            ({ samples, trigger }) =>
                new Float32Array(
                    trigger
                        ? Math.min(2 * samples, VIDEO_HISTORY_LEN)
                        : samples,
                ),
        );
        this.windows = histories.map(
            ({ samples }) => new Float32Array(samples),
        );
        return forgotten;
    }

    /** Forgets every signal's samples, as when the engine starts playing again. */
    clear(): void {
        this.streams.clear();
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
        const has = (tap: number) => this.streams.get(tap)?.hasData === true;
        return (
            program.tapSlots.every(({ tap }) => has(tap)) &&
            program.histories.every(({ tap }) => has(tap))
        );
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
        program.histories.forEach(({ tap, trigger }, row) => {
            const stream = this.streams.get(tap);
            if (stream === undefined) return;
            const recent = this.scratch[row];
            stream.recent(recent);
            this.history.write(
                row,
                alignWindow(recent, this.windows[row], trigger),
            );
        });
    }

    /**
     * How many samples at the end of untriggered row `row` are real; the rest,
     * from before the signal's first sample, read as silence.
     */
    filled(row: number): number {
        const history = this.histories[row];
        if (history === undefined) return 0;
        return Math.min(
            history.samples,
            this.streams.get(history.tap)?.available ?? 0,
        );
    }

    destroy(): void {
        this.history.destroy();
    }
}
