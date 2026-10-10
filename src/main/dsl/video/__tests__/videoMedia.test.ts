import { describe, expect, it } from 'vitest';
import schemas from '@modular/core/schemas.json';
import { executePatchScript } from '../../executor';
import { exec } from './videoExec';

describe('$v media sources', () => {
    describe('image and video', () => {
        const wgslOf = (source: string) => exec(source).video!.wgsl;

        it('samples a workspace image at the coordinate being drawn', () => {
            const { video } = exec(`$v.out($v.image('pictures/photo.png'));`);
            expect(video!.sources).toEqual([
                { kind: 'image', path: 'pictures/photo.png' },
            ]);
            expect(video!.wgsl).toContain(
                '@group(0) @binding(1) var fb_sampler: sampler;',
            );
            expect(video!.wgsl).toContain(
                '@group(0) @binding(2) var src_0: texture_2d<f32>;',
            );
            expect(video!.wgsl).toContain('video_source(src_0, uv, 1)');
        });

        it('shares a binding between uses of one file and numbers different files', () => {
            const { video } = exec(`
                const a = $v.image('a.png');
                const b = $v.video('b.mp4', { fit: 'contain' });
                $v.out($v.mix(a, b, 0.5).$.add($v.image('a.png', { fit: 'stretch' })));
            `);
            expect(video!.sources).toEqual([
                { kind: 'image', path: 'a.png' },
                { kind: 'video', path: 'b.mp4' },
            ]);
            expect(video!.wgsl).toContain('video_source(src_1, uv, 2)');
            expect(video!.wgsl).toContain('video_source(src_0, uv, 0)');
        });

        it('gives a video its speed and loop points', () => {
            const { video } = exec(`
                $v.video('a.mp4', { speed: 0.5, loop: [1, 2.5] }).out();
            `);
            expect(video!.sources).toEqual([
                {
                    kind: 'video',
                    loopEnd: 2.5,
                    loopStart: 1,
                    path: 'a.mp4',
                    speed: 0.5,
                },
            ]);
        });

        it('leaves out settings that are the defaults', () => {
            const { video } = exec(`
                $v.video('a.mp4', { speed: 1, loop: [0] }).out();
            `);
            expect(video!.sources).toEqual([{ kind: 'video', path: 'a.mp4' }]);
        });

        it('loops from a start to the end of the file', () => {
            const { video } = exec(`$v.video('a.mp4', { loop: [2] }).out();`);
            expect(video!.sources).toEqual([
                { kind: 'video', loopStart: 2, path: 'a.mp4' },
            ]);
        });

        it('plays one file twice when the settings differ', () => {
            const { video } = exec(`
                $v.mix($v.video('a.mp4'), $v.video('a.mp4', { speed: 2 }), 0.5).out();
            `);
            expect(video!.sources).toHaveLength(2);
            const same = exec(`
                $v.mix($v.video('a.mp4', { speed: 2 }), $v.video('a.mp4', { speed: 2 }), 0.5).out();
            `);
            expect(same.video!.sources).toHaveLength(1);
        });

        it('rejects a speed or loop it cannot play', () => {
            for (const speed of ['-1', '17', "'fast'", 'NaN']) {
                expect(() =>
                    exec(`$v.video('a.mp4', { speed: ${speed} });`),
                ).toThrow(/\$v\.video: speed must be a number from 0 to 16/);
            }
            for (const loop of [
                '[2, 1]',
                '[1, 1]',
                '[-1]',
                '[]',
                '[0, 1, 2]',
                '3',
                "['a']",
            ]) {
                expect(() =>
                    exec(`$v.video('a.mp4', { loop: ${loop} });`),
                ).toThrow(
                    /\$v\.video: loop must be \[start\] or \[start, end\]/,
                );
            }
        });

        it('rejects speed and loop on an image', () => {
            expect(() => exec(`$v.image('a.png', { speed: 2 });`)).toThrow(
                /\$v\.image: speed and loop apply only to video/,
            );
        });

        it('binds media after the feedback and audio history textures', () => {
            const { video } = exec(`
                const trail = $v.buffer();
                const wave = $v.fromAudio($sine('110hz'));
                $v.image('a.png').$.add(trail.read().$.mult(0.9)).write(trail).$.mult($v.hsv(wave)).out();
            `);
            expect(video!.wgsl).toContain('var fb_0: texture_2d<f32>');
            expect(video!.wgsl).toContain(
                '@group(0) @binding(3) var history_tex',
            );
            expect(video!.wgsl).toContain('@group(0) @binding(4) var src_0');
        });

        it('moves with the warps like any other pattern', () => {
            const wgsl = wgslOf(`$v.image('a.png').$.kaleid(6).out();`);
            expect(wgsl).toMatch(
                /fn f0\(uv: vec2f\) -> vec3f \{\n    return video_source\(src_0/,
            );
        });

        it('declares no media for a patch without any', () => {
            const wgsl = wgslOf(`$v.out($v.hsv(0.5));`);
            expect(wgsl).not.toContain('src_0');
            expect(wgsl).not.toContain('fb_sampler');
        });

        it('rejects a bad path, extension or fit', () => {
            expect(() => exec(`$v.image(3);`)).toThrow(
                /\$v\.image: path must be a string/,
            );
            expect(() => exec(`$v.image('/etc/photo.png');`)).toThrow(
                /path must stay inside the workspace folder/,
            );
            expect(() => exec(`$v.image('../photo.png');`)).toThrow(
                /path must stay inside the workspace folder/,
            );
            expect(() => exec(`$v.image('notes.txt');`)).toThrow(
                /"notes\.txt" must be one of \.png/,
            );
            expect(() => exec(`$v.video('clip.png');`)).toThrow(
                /\$v\.video: "clip\.png" must be one of \.mp4/,
            );
            expect(() => exec(`$v.image('a.png', { fit: 'zoom' });`)).toThrow(
                /\$v\.image: fit must be one of cover, contain, stretch/,
            );
        });

        it('names a file the workspace does not have', () => {
            expect(() =>
                executePatchScript(
                    `$v.image('missing.png');`,
                    schemas as never,
                    {
                        mediaExists: () => false,
                        sampleRate: 48_000,
                        workspaceRoot: '/workspace',
                    },
                ),
            ).toThrow(
                /\$v\.image: no file "missing\.png" in the workspace folder/,
            );
        });
    });

    describe('video audio', () => {
        const audioModule = (source: string) => {
            const { patch } = exec(source) as unknown as {
                patch: {
                    modules: {
                        id: string;
                        moduleType: string;
                        params: Record<string, unknown>;
                    }[];
                };
            };
            return patch.modules.filter((m) => m.moduleType === '_mediaAudio');
        };

        it('plays the sound of a video as an audio signal', () => {
            const modules = audioModule(
                `$v.video('clips/loop.mp4').audio.out();`,
            );
            expect(modules).toHaveLength(1);
            expect(modules[0].params.wav).toMatchObject({
                channels: 2,
                path: 'media:clips/loop.mp4',
                type: 'wav_ref',
            });
        });

        it('keeps the audio on what out returns', () => {
            const modules = audioModule(
                `$v.video('clips/loop.mp4', { loop: [1], fit: 'contain' }).out().audio.out();`,
            );
            expect(modules).toHaveLength(1);
        });

        it('makes one player however often the audio is read', () => {
            const modules = audioModule(`
                const clip = $v.video('clips/loop.mp4');
                clip.audio.out();
                clip.audio.out();
            `);
            expect(modules).toHaveLength(1);
        });

        it('makes no player for a video whose audio is not read', () => {
            expect(
                audioModule(`$v.video('clips/loop.mp4').out();`),
            ).toHaveLength(0);
        });

        it('plays at the speed and between the loop points of the video', () => {
            const [player] = audioModule(
                `$v.video('clips/loop.mp4', { speed: 0.5, loop: [1, 2.5] }).audio.out();`,
            );
            expect(player.params).toMatchObject({
                loopEnd: 2.5,
                loopStart: 1,
                speed: 0.5,
            });
        });

        it('plays the whole track at normal speed by default', () => {
            const [player] = audioModule(
                `$v.video('clips/loop.mp4').audio.out();`,
            );
            expect(player.params).toMatchObject({ loopStart: 0, speed: 1 });
            expect(player.params).not.toHaveProperty('loopEnd');
        });

        it('says so when a video has no sound', () => {
            expect(() =>
                exec(`$v.video('clips/silent.mp4').audio.out();`),
            ).toThrow(
                /\$v\.video\(\.\.\.\)\.audio: "clips\/silent\.mp4" has no audio/,
            );
        });

        it('leaves a video without sound alone while its audio is unread', () => {
            expect(() =>
                exec(`$v.video('clips/silent.mp4').out();`),
            ).not.toThrow();
        });

        it('gives only a video file an audio property', () => {
            const { video } = exec(`
                const stream = $v.stream('https://a.example/b.mp4');
                const image = $v.image('pictures/a.png');
                if (stream.audio !== undefined || image.audio !== undefined) {
                    throw new Error('has audio');
                }
                $v.out(stream);
            `);
            expect(video).not.toBeNull();
        });

        it('keeps the audio after a chain is taken from the video, not on the chain', () => {
            const { video } = exec(`
                const clip = $v.video('clips/loop.mp4');
                if (clip.$.invert().audio !== undefined) throw new Error('chained audio');
                $v.out(clip);
            `);
            expect(video).not.toBeNull();
        });
    });

    describe('camera and screen', () => {
        it('draws a camera as a color source with no file', () => {
            const { video } = exec(`$v.camera().out();`);
            expect(video!.sources).toEqual([{ kind: 'camera', path: '' }]);
            expect(video!.wgsl).toContain('video_source(src_0, uv, 1)');
        });

        it('chooses a camera by name and a display by number', () => {
            expect(
                exec(`$v.camera({ device: 'FaceTime' }).out();`).video!.sources,
            ).toEqual([{ device: 'FaceTime', kind: 'camera', path: '' }]);
            expect(
                exec(`$v.screen({ display: 2 }).out();`).video!.sources,
            ).toEqual([{ display: 2, kind: 'screen', path: '' }]);
            expect(exec(`$v.screen().out();`).video!.sources).toEqual([
                { kind: 'screen', path: '' },
            ]);
        });

        it('shares a source between uses of one camera and splits different ones', () => {
            const same = exec(`
                $v.mix($v.camera(), $v.camera({ fit: 'contain' }), 2.5).out();
            `);
            expect(same.video!.sources).toHaveLength(1);
            const different = exec(`
                $v.mix($v.camera({ device: 'a' }), $v.camera({ device: 'b' }), 2.5).out();
            `);
            expect(different.video!.sources).toHaveLength(2);
        });

        it('takes the fit option as images do', () => {
            expect(
                exec(`$v.camera({ fit: 'stretch' }).out();`).video!.wgsl,
            ).toContain('video_source(src_0, uv, 0)');
            expect(() => exec(`$v.screen({ fit: 'zoom' });`)).toThrow(
                /\$v\.screen: fit must be one of cover, contain, stretch/,
            );
        });

        it('rejects a camera name or display that cannot be right', () => {
            expect(() => exec(`$v.camera({ device: '' });`)).toThrow(
                /\$v\.camera: device must be part of a camera's name/,
            );
            expect(() => exec(`$v.camera({ device: 3 });`)).toThrow(
                /\$v\.camera: device must be part of a camera's name/,
            );
            for (const display of ['0', '1.5', "'two'"]) {
                expect(() =>
                    exec(`$v.screen({ display: ${display} });`),
                ).toThrow(/\$v\.screen: display must be a whole number from 1/);
            }
        });
    });

    describe('network addresses', () => {
        it('streams a video from an address, with no file or extension to check', () => {
            const { video } = exec(
                `$v.stream('https://cdn.example.com/live/stream?id=7').out();`,
            );
            expect(video!.sources).toEqual([
                {
                    kind: 'video',
                    path: 'https://cdn.example.com/live/stream?id=7',
                },
            ]);
        });

        it('draws an image from an address', () => {
            const { video } = exec(
                `$v.image('http://example.com/pictures/photo').out();`,
            );
            expect(video!.sources).toEqual([
                { kind: 'image', path: 'http://example.com/pictures/photo' },
            ]);
        });

        it('uses one source for an address written two ways', () => {
            const { video } = exec(`
                $v.mix($v.stream('HTTPS://CDN.example.com/a.mp4'), $v.stream('https://cdn.example.com/a.mp4'), 2.5).out();
            `);
            expect(video!.sources).toHaveLength(1);
        });

        it('fits a stream like any other source', () => {
            expect(() =>
                exec(
                    `$v.stream('https://a.example/b.m3u8', { fit: 'contain' }).out();`,
                ),
            ).not.toThrow();
            expect(() =>
                exec(
                    `$v.stream('https://a.example/b.m3u8', { fit: 'tile' }).out();`,
                ),
            ).toThrow(/fit must be one of/);
        });

        it('sends an address given to $v.video to $v.stream', () => {
            expect(() => exec(`$v.video('https://a.example/b.mp4');`)).toThrow(
                /\$v\.video: plays files in the workspace folder; use \$v\.stream/,
            );
        });

        it('refuses protocols a browser cannot play, and says what works', () => {
            for (const url of [
                'rtsp://camera.local/stream',
                'rtmp://live.example/app',
                'ftp://files.example/a.mp4',
                'file:///etc/a.mp4',
            ]) {
                expect(() => exec(`$v.stream('${url}');`)).toThrow(
                    /\$v\.stream: [a-z]+:\/\/ addresses cannot be played; use an http or https URL/,
                );
            }
        });

        it('rejects a stream that is not an address', () => {
            expect(() => exec(`$v.stream('clip.mp4');`)).toThrow(
                /\$v\.stream: url must be an http or https address/,
            );
            expect(() => exec(`$v.stream('http://');`)).toThrow(
                /\$v\.stream: "http:\/\/" is not a valid URL/,
            );
        });

        it('still keeps files inside the workspace', () => {
            expect(() => exec(`$v.video('../a.mp4');`)).toThrow(
                /must stay inside the workspace folder/,
            );
        });
    });
});
