import { describe, expect, it } from 'vitest';
import { exec } from './videoExec';

describe('$v memory and previews', () => {
    describe('feedback', () => {
        it('compiles an update function into a read and a write of one buffer', () => {
            const { video } = exec(`
                const seed = $v.shape($v.ramp(), $v.ramp('v'), 0.1);
                const trail = $v.feedback(
                    (prev) => $v.mix($v.hsv($v.time, 1, seed), prev, 0.9),
                    { zoom: 1.02, rotate: 0.005, edge: 'mirror' },
                );
                $v.out(trail);
            `);
            expect(video!.feedbackBufferCount).toBe(1);
            expect(video!.wgsl).toContain('fb_0');
            expect(video!.wgsl).toContain('feedback_mirror');
        });

        it('numbers independent loops separately', () => {
            const { video } = exec(`
                const a = $v.feedback((prev) => $v.mix($v.hsv(0.1), prev, 0.5));
                const b = $v.feedback((prev) => $v.mix($v.hsv(0.6), prev, 0.5));
                $v.out($v.mix(a, b));
            `);
            expect(video!.feedbackBufferCount).toBe(2);
        });

        it('lets a slider drive the transform', () => {
            const { video } = exec(`
                $v.out($v.feedback(
                    (prev) => $v.mix($v.hsv(0.3), prev, 0.9),
                    { zoom: $slider('Zoom', 1, 0.9, 1.1) },
                ));
            `);
            expect(
                video!.uniforms.map((u) =>
                    u.kind === 'control' ? u.moduleId : u.kind,
                ),
            ).toEqual(['__slider_Zoom']);
        });

        it('keeps a loop whose result is never read from the output', () => {
            const { video } = exec(`
                $v.feedback((prev) => $v.mix($v.hsv(0.3), prev, 0.9));
                $v.out($v.hsv(0.5));
            `);
            expect(video!.feedbackBufferCount).toBe(1);
        });

        it('accepts an update that returns a field, as gray', () => {
            const video = exec(`$v.feedback((prev) => $v.ramp()).out();`).video;
            expect(video!.feedbackBufferCount).toBe(1);
        });

        it('rejects an update that returns something else', () => {
            expect(() => exec(`$v.feedback((prev) => 'red');`)).toThrow(
                /\$v\.feedback: update must return a video field or color/,
            );
        });

        it('rejects a non-function update', () => {
            expect(() => exec(`$v.feedback($v.hsv(0));`)).toThrow(
                /\$v\.feedback: update must be a function/,
            );
        });

        it('rejects more loops than there are render targets', () => {
            const loops = Array.from(
                { length: 8 },
                () => `$v.feedback((p) => $v.mix($v.hsv(0), p, 0.5));`,
            ).join('\n');
            expect(() => exec(loops)).toThrow(/at most 7 feedback loops/);
        });
    });

    describe('frameDelay', () => {
        it('holds a frame back through one buffer per frame', () => {
            const { video } = exec(`
                $v.out($v.frameDelay($v.hsv($v.ramp()), 3));
            `);
            expect(video!.feedbackBufferCount).toBe(3);
        });

        it('delays by a frame by default, and shifts each stage into the next', () => {
            const wgsl = exec(`$v.out($v.frameDelay($v.hsv($v.ramp())));`)
                .video!.wgsl;
            expect(wgsl).toContain('textureSampleLevel(fb_0');
            expect(
                exec(`$v.out($v.frameDelay($v.hsv($v.ramp())));`).video!
                    .feedbackBufferCount,
            ).toBe(1);
        });

        it('turns a field into a gray color', () => {
            const { video } = exec(`$v.out($v.frameDelay($v.ramp(), 2));`);
            expect(video!.feedbackBufferCount).toBe(2);
        });

        it('chains the way the function is called', () => {
            const chained = exec(`$v.hsv($v.ramp()).$.frameDelay(2).out();`)
                .video!.wgsl;
            const called = exec(`$v.out($v.frameDelay($v.hsv($v.ramp()), 2));`)
                .video!.wgsl;
            expect(chained).toBe(called);
        });

        it('shares the frame buffers with feedback loops and buffers', () => {
            expect(() =>
                exec(`
                    const a = $v.frameDelay($v.hsv($v.ramp()), 6);
                    $v.out($v.feedback((prev) => $v.mix(a, prev, 2.5)));
                    $v.buffer();
                `),
            ).toThrow(/at most 7 feedback loops/);
        });

        it('rejects a delay that is not a whole number from 1 to 7', () => {
            for (const frames of ['0', '8', '1.5', "'two'"]) {
                expect(() =>
                    exec(`$v.frameDelay($v.hsv(0), ${frames});`),
                ).toThrow(
                    /\$v\.frameDelay: frames must be a whole number from 1 to 7/,
                );
            }
        });
    });

    describe('previews', () => {
        it('records each preview call with its view and source line', () => {
            const result = exec(`
                const wave = $v.preview($v.osc($v.ramp(), 4));
                $v.out($v.preview($v.hsv(wave), { view: 'waveform' }));
            `);
            expect(
                result.videoPreviews.map((p) => [
                    p.view,
                    p.sourceLocation?.line,
                ]),
            ).toEqual([
                ['image', expect.any(Number)],
                ['waveform', expect.any(Number)],
            ]);
            const [first, second] = result.videoPreviews;
            expect(second.sourceLocation!.line).toBe(
                first.sourceLocation!.line + 1,
            );
            expect(result.video!.previewCount).toBe(2);
        });

        it('returns the signal it previews', () => {
            const { video } = exec(`
                $v.out($v.colorize($v.preview($v.ramp()), 0, 0));
            `);
            expect(video!.previewCount).toBe(1);
        });

        it('shows black for a patch that only previews', () => {
            const { video } = exec(`$v.preview($v.hsv($v.ramp()));`);
            expect(video).not.toBeNull();
            expect(video!.previewCount).toBe(1);
        });

        it('keeps the nodes a preview reads even when the output ignores them', () => {
            const { video } = exec(`
                $v.preview($v.osc($v.ramp(), 9));
                $v.out($v.hsv(0.5));
            `);
            expect(video!.wgsl).toContain('fn preview_0(');
            expect(video!.wgsl).toContain('fract(');
        });

        it('rejects an unknown view', () => {
            expect(() =>
                exec(`$v.preview($v.ramp(), { view: 'histogram' });`),
            ).toThrow(
                /\$v\.preview: view must be one of image, waveform, vectorscope/,
            );
        });

        it('rejects something that is not a video signal', () => {
            expect(() => exec(`$v.preview(0.5);`)).toThrow(
                /\$v\.preview: signal must be a video field or color/,
            );
        });

        it('reports no previews for a patch without any', () => {
            expect(exec(`$v.out($v.hsv(0));`).videoPreviews).toEqual([]);
        });
    });
});
