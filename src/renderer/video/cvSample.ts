import type {
    VideoCvSample,
    VideoPreviewFrame,
} from '../../shared/video/videoGraph';

/**
 * Average brightness of the pixels of `frame` inside the region
 * `sample` names, in volts: 0 for black to 5 for white. A region is clipped to
 * the frame and always covers at least one pixel.
 */
export function regionAverage(
    frame: VideoPreviewFrame,
    sample: Pick<VideoCvSample, 'x' | 'y' | 'size'>,
): number {
    const { data, width, height } = frame;
    const centerX = sample.x * width;
    const centerY = (1 - sample.y) * height;
    const clip = (value: number, max: number) =>
        Math.min(max, Math.max(0, value));
    const left = Math.floor(clip(centerX - sample.size * width, width - 1));
    const right = Math.ceil(clip(centerX + sample.size * width, width));
    const top = Math.floor(clip(centerY - sample.size * height, height - 1));
    const bottom = Math.ceil(clip(centerY + sample.size * height, height));

    let sum = 0;
    let count = 0;
    for (let y = top; y < Math.max(bottom, top + 1); y++) {
        for (let x = left; x < Math.max(right, left + 1); x++) {
            const i = (y * width + x) * 4;
            sum += 0.299 * data[i] + 0.587 * data[i + 1] + 0.114 * data[i + 2];
            count++;
        }
    }
    return (sum / count / 255) * 5;
}
