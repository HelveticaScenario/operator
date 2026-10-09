import { describe, expect, it } from 'vitest';
import schemas from '@modular/core/schemas.json';
import { executePatchScript } from '../../executor';
import { buildLibSource } from '../../typescriptLibGen';
import { VIDEO_DOCS } from '../../../../shared/dsl/videoDocs';

const exec = (source: string) =>
    executePatchScript(source, schemas as never, {
        sampleRate: 48_000,
        workspaceRoot: '/workspace',
    });

describe('$v in the DSL executor', () => {
    it('returns no video shader when the patch never calls $v.out', () => {
        expect(exec(`$v.ramp();`).video).toBeNull();
    });

    it('compiles the graph reachable from $v.out', () => {
        const { video } = exec(`
            const x = $v.ramp('h');
            const wave = $v.osc(x, 4, $v.time, { shape: 'triangle' });
            $v.ramp('v');
            $v.out($v.colorize(wave, 0.5, 1));
        `);
        expect(video).not.toBeNull();
        expect(video!.wgsl).toContain('abs(2.0 * fract(');
        expect(video!.wgsl).not.toContain('uv.y');
    });

    it('rejects a field passed where a color is required', () => {
        expect(() => exec(`$v.out($v.ramp());`)).toThrow(
            /\$v\.out: input must be a video color/,
        );
    });

    it('rejects a non-numeric field argument', () => {
        expect(() => exec(`$v.osc($v.ramp(), 'fast');`)).toThrow(
            /\$v\.osc: freq must be a number or a video field/,
        );
    });

    it('reports an invalid enumerated option', () => {
        expect(() => exec(`$v.out($v.colorize($v.ramp('z'), 0, 0));`)).toThrow(
            /param "axis" must be one of/,
        );
    });

    describe('math and keying modules', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('uses the field variant for fields and the color variant for colors', () => {
            const fields = wgslOf(
                `$v.out($v.colorize($v.mult($v.ramp(), 0.5), 0, 0));`,
            );
            expect(fields).toContain('let v1: f32 = v0 * 0.5;');
            const colors = wgslOf(`
                const c = $v.hsv($v.ramp());
                $v.out($v.mult(c, $v.ramp('v')));
            `);
            expect(colors).toMatch(/let v\d: vec3f = v\d \* v\d;/);
        });

        it('promotes a field operand of a color operation to gray', () => {
            const wgsl = wgslOf(`
                $v.out($v.add($v.colorize(1, 0, 0), 0.25));
            `);
            expect(wgsl).toContain('vec3f(0.25, 0.25, 0.25)');
        });

        it('declares each module helper once however often the module is used', () => {
            const wgsl = wgslOf(`
                const a = $v.shape($v.ramp(), $v.ramp('v'), 0.2);
                const b = $v.shape($v.ramp(), $v.ramp('v'), 0.1, 0, { shape: 'box' });
                $v.out($v.key($v.hsv(a), $v.procAmp($v.hsv(b)), $v.comparator(a)));
            `);
            expect(wgsl.match(/fn shape_edge/g)).toHaveLength(1);
            expect(wgsl.match(/fn hsv_to_rgb/g)).toHaveLength(1);
            expect(wgsl).toContain('fn procamp');
        });

        it('rejects a color where a field is required', () => {
            expect(() => exec(`$v.comparator($v.colorize(0, 0, 0));`)).toThrow(
                /\$v\.comparator: input must be a number or a video field/,
            );
            expect(() =>
                exec(`$v.mix($v.ramp(), $v.ramp(), $v.colorize(0, 0, 0));`),
            ).toThrow(/\$v\.mix: amount must be a number or a video field/);
        });

        it('rejects an invalid shape option', () => {
            expect(() =>
                exec(
                    `$v.out($v.hsv($v.shape(0.5, 0.5, 0.2, 0, { shape: 'star' })));`,
                ),
            ).toThrow(/param "shape" must be one of circle, box, diamond/);
        });
    });

    describe('control-bound inputs', () => {
        it('binds a slider to a uniform slot carrying its current value', () => {
            const { video } = exec(`
                const freq = $slider('Freq', 4, 1, 10);
                $v.out($v.colorize($v.osc($v.ramp(), freq), 0, 0));
            `);
            expect(video!.uniforms).toEqual([
                {
                    kind: 'control',
                    slot: 0,
                    moduleId: '__slider_Freq',
                    value: 4,
                },
            ]);
            expect(video!.wgsl).toContain('u.slots[0][0]');
        });

        it('shares one slot between every use of a control', () => {
            const { video } = exec(`
                const amt = $slider('Amt', 0.5, 0, 1);
                const a = $v.osc($v.ramp(), 3, amt);
                $v.out($v.colorize(a, amt, $slider('Other', 1, 0, 2)));
            `);
            expect(
                video!.uniforms.map((u) =>
                    u.kind === 'control' ? u.moduleId : u.kind,
                ),
            ).toEqual(['__slider_Amt', '__slider_Other']);
        });

        it('binds a button as 0 until pressed', () => {
            const { video } = exec(`
                $v.out($v.colorize($btn('Flash'), 0, 0));
            `);
            expect(video!.uniforms).toEqual([
                {
                    kind: 'control',
                    slot: 0,
                    moduleId: '__button_Flash',
                    value: 0,
                },
            ]);
        });
    });

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

        it('rejects an update that returns a field', () => {
            expect(() => exec(`$v.feedback((prev) => $v.ramp());`)).toThrow(
                /\$v\.feedback: update must return a video color/,
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

    describe('audio-signal inputs', () => {
        const taps = (source: string) =>
            exec(source).patch.modules.filter(
                (m) => m.moduleType === '_videoTap',
            );

        it('publishes an audio signal through a tap bound to a uniform slot', () => {
            const result = exec(`
                $v.out($v.colorize($sine('1hz').range(0, 1), 0, 0));
            `);
            expect(result.video!.uniforms).toEqual([
                { kind: 'tap', slot: 0, tap: 0, value: 0 },
            ]);
            expect(result.video!.wgsl).toContain('u.slots[0][0]');
            const [tap] = result.patch.modules.filter(
                (m) => m.moduleType === '_videoTap',
            );
            expect(tap.params).toMatchObject({ slot: 0 });
        });

        it('shares one tap between every use of a signal', () => {
            const found = taps(`
                const lfo = $sine('1hz').range(0, 1);
                $v.out($v.colorize(lfo, $v.osc($v.ramp(), lfo), 0));
            `);
            expect(found).toHaveLength(1);
        });

        it('gives different signals different taps', () => {
            const { video } = exec(`
                const a = $sine('1hz').range(0, 1);
                const b = $saw('2hz').range(0, 1);
                $v.out($v.colorize(a, b, 0));
            `);
            expect(
                video!.uniforms.map((u) => u.kind === 'tap' && u.tap),
            ).toEqual([0, 1]);
        });

        it('uses a control directly rather than through a tap', () => {
            expect(
                taps(`$v.out($v.colorize($slider('Level', 1, 0, 1), 0, 0));`),
            ).toHaveLength(0);
        });

        it('rejects a polyphonic signal', () => {
            expect(() =>
                exec(
                    `$v.out($v.colorize($c($sine('1hz'), $sine('2hz')), 0, 0));`,
                ),
            ).toThrow(/\$v\.colorize: r has 2 channels/);
        });

        it('rejects more signals than the engine has taps', () => {
            const reads = Array.from(
                { length: 65 },
                (_, i) => `$v.osc($v.ramp(), $sine(${i + 1}))`,
            ).join(', ');
            expect(() =>
                exec(`const f = [${reads}]; $v.out($v.colorize(f[0], 0, 0));`),
            ).toThrow(/at most 64 audio signals/);
        });

        it('keeps the internal tap module out of the user namespace', () => {
            expect(() => exec(`_videoTap($sine('1hz'), 0);`)).toThrow();
        });
    });

    it('does not declare the internal tap module in the DSL typings', () => {
        expect(buildLibSource(schemas as never, null)).not.toContain(
            '_videoTap',
        );
    });

    describe('coordinate ramps and waveshaping', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('compiles an untransformed ramp to the bare frame coordinate', () => {
            const wgsl = wgslOf(`$v.out($v.colorize($v.ramp(), 0, 0));`);
            expect(wgsl).toContain('let v0: f32 = uv.x;');
        });

        it('routes a transformed ramp through video_transform', () => {
            const wgsl = wgslOf(
                `$v.out($v.colorize($v.ramp('v', { rotate: 0.1, zoom: 2 }), 0, 0));`,
            );
            expect(wgsl).toContain(
                'video_transform(uv, 2.0, 0.1, vec2f(0.0, 0.0)).y',
            );
        });

        it('offers radial and angular ramps', () => {
            const wgsl = wgslOf(
                `$v.out($v.colorize($v.ramp('r'), $v.ramp('a'), 0));`,
            );
            expect(wgsl).toContain('ramp_radius(uv)');
            expect(wgsl).toContain('ramp_angle(uv)');
        });

        it('declares the shared transform once when ramps and feedback both use it', () => {
            const wgsl = wgslOf(`
                const r = $v.ramp('h', { rotate: 0.1 });
                $v.out($v.feedback((prev) => $v.mix($v.hsv(r), prev, 0.9), { rotate: 0.01 }));
            `);
            expect(wgsl.match(/fn video_transform/g)).toHaveLength(1);
        });

        it('lets a slider drive a ramp transform', () => {
            const { video } = exec(`
                const spin = $slider('Spin', 0, -0.5, 0.5);
                $v.out($v.hsv($v.ramp('a', { rotate: spin })));
            `);
            expect(video!.uniforms).toHaveLength(1);
        });

        it('compiles min, max, wrap and fold', () => {
            const wgsl = wgslOf(`
                const x = $v.ramp();
                const y = $v.ramp('v');
                $v.out($v.colorize(
                    $v.max($v.wrap(x, 3), $v.fold(y, 4)),
                    $v.min(x, y),
                    0,
                ));
            `);
            expect(wgsl).toContain('fract(');
            expect(wgsl).toMatch(/max\(v\d, v\d\)/);
            expect(wgsl).toMatch(/min\(v\d, v\d\)/);
        });

        it('applies min and max per channel to colors', () => {
            const wgsl = wgslOf(
                `$v.out($v.max($v.hsv($v.ramp()), $v.hsv($v.ramp('v'))));`,
            );
            expect(wgsl).toMatch(/let v\d: vec3f = max\(v\d, v\d\);/);
        });

        it('rejects an unknown ramp axis', () => {
            expect(() =>
                exec(`$v.out($v.colorize($v.ramp('q'), 0, 0));`),
            ).toThrow(/param "axis" must be one of h, v, d, r, a/);
        });
    });

    it('documents exactly the members of $v', () => {
        let members: string[] = [];
        try {
            exec(`throw new Error('members:' + Object.keys($v).join(','));`);
        } catch (error) {
            members = (error as Error).message.split('members:')[1].split(',');
        }
        expect(members.sort()).toEqual(VIDEO_DOCS.map((d) => d.name).sort());
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

    describe('toCV', () => {
        it('returns a 0..1 audio signal fed by a region of the video', () => {
            const result = exec(`
                const level = $v.toCV($v.ramp(), { x: 0.25, y: 0.75, size: 0.1 });
                $sine(level.range(100, 200)).out();
                $v.out($v.hsv(0.5));
            `);
            expect(result.video!.cvSamples).toEqual([
                { id: '__videoCV_0', index: 0, size: 0.1, x: 0.25, y: 0.75 },
            ]);
            const cv = result.patch.modules.find((m) => m.id === '__videoCV_0');
            expect(cv?.moduleType).toBe('$signal');
        });

        it('defaults to the whole frame', () => {
            const { video } = exec(`
                $v.toCV($v.ramp());
                $v.out($v.hsv(0.5));
            `);
            expect(video!.cvSamples[0]).toMatchObject({
                size: 0.5,
                x: 0.5,
                y: 0.5,
            });
        });

        it('numbers its preview slots among the editor previews', () => {
            const { video, videoPreviews } = exec(`
                $v.preview($v.ramp());
                $v.toCV($v.ramp('v'));
                $v.out($v.preview($v.hsv(0.5), { view: 'vectorscope' }));
            `);
            expect(videoPreviews.map((p) => p.index)).toEqual([0, 2]);
            expect(video!.cvSamples.map((c) => c.index)).toEqual([1]);
            expect(video!.previewCount).toBe(3);
        });

        it('closes the loop: video drives audio drives video', () => {
            const { video } = exec(`
                const level = $v.toCV($v.ramp(), { x: 0.9, size: 0.02 });
                $v.out($v.colorize(level, level, level));
            `);
            expect(video!.uniforms.map((u) => u.kind)).toEqual(['tap']);
            expect(video!.cvSamples).toHaveLength(1);
        });

        it('rejects a region that is empty or not finite', () => {
            expect(() => exec(`$v.toCV($v.ramp(), { size: 0 });`)).toThrow(
                /\$v\.toCV: size must be greater than 0/,
            );
            expect(() => exec(`$v.toCV($v.ramp(), { x: 'left' });`)).toThrow(
                /\$v\.toCV: x must be a finite number/,
            );
        });

        it('rejects something that is not a video signal', () => {
            expect(() => exec(`$v.toCV(0.5);`)).toThrow(
                /\$v\.toCV: signal must be a video field or color/,
            );
        });
    });

    describe('noise', () => {
        it('compiles to value noise of its three coordinates', () => {
            const { video } = exec(`
                $v.out($v.hsv($v.noise($v.mult($v.ramp(), 4), $v.ramp('v'), $v.time)));
            `);
            expect(video!.wgsl).toContain('fn noise_hash(');
            expect(video!.wgsl).toContain('fn noise_value(');
            expect(video!.wgsl).toMatch(
                /noise_value\(vec3f\(v\d, v\d, u\.time\)\)/,
            );
        });

        it('takes a constant z by default', () => {
            const { video } = exec(`$v.out($v.hsv($v.noise(0.5, 0.5)));`);
            expect(video!.wgsl).toContain('noise_value(vec3f(0.5, 0.5, 0.0))');
        });

        it('rejects a color coordinate', () => {
            expect(() => exec(`$v.noise($v.hsv(0), 0);`)).toThrow(
                /\$v\.noise: x must be a number or a video field/,
            );
        });
    });
});
