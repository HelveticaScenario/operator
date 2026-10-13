/**
 * Documentation for the `$v` video namespace. One table feeds both the
 * generated Monaco typings (JSDoc plus declarations) and the Help window's
 * Video page, so the two cannot drift.
 */

export interface VideoDocParam {
    name: string;
    description: string;
}

export interface VideoDoc {
    /** Member of `$v`, e.g. `ramp` for `$v.ramp`. */
    name: string;
    group: string;
    /** Paragraphs separated by a blank line. */
    description: string;
    params: VideoDocParam[];
    /** Complete patches; each must run as written. */
    examples: string[];
    /** TypeScript member declarations, one per overload. */
    declarations: string[];
}

/** Section order on the Video help page. */
export const VIDEO_GROUPS = [
    'Generators',
    'Warping',
    'Filters',
    'Color',
    'Post',
    'Math',
    'Shaping',
    'Compositing',
    'Memory',
    'Output',
];

export const VIDEO_INTRO = {
    description:
        'Video synthesis. Patches build a graph of signals that is drawn per pixel behind the code in the editor when a patch calls `$v.out`. View, Toggle Performance Window (Ctrl+Shift+V) opens a second window that shows the same picture for an audience; while it is open, the picture behind the code takes the shape and resolution of that window. Video signals cannot be connected to audio inputs.\n\nEvery video signal is a color with an alpha at each pixel. A number, such as the frequency of an oscillator or a mix amount, can be given a signal too, and reads its brightness; a gray signal, such as an oscillator or a ramp, gives the same value in every channel. The generators draw at the position of each pixel, so `$v.osc(10)` already varies across the frame; their config takes other coordinates when you want them. The picture shows a signal laid over black by its alpha.\n\nSliders, buttons and audio signals can be passed anywhere a number is accepted, and the picture follows them live. So can patterns from `$p(...)`, `$p.s(...)` and `$p.arrange(...)`, which play through their own `$cycle` as they do for any audio parameter; their values are used as they are, so `$p("0 0.25 0.5 0.75")` steps through four hues a cycle. Audio signals are sampled about 60 times a second and used in volts as the audio graph produces them, so scale them to the range an input expects, for example `.range(0, 1)`; `.range(min, max)` on a video signal sets any other range, such as `.range(1, 3)` for a zoom of one to three times. A signal of several channels, such as a chord or a stereo pair, is summed to one.',
    examples: [
        '$v.osc(10).$.kaleid(6).$.hsv().out()',
        "$v.out($v.hsv($slider('Hue', 0.3, 0, 1), 1, $slider('Level', 1, 0, 1)))",
        "$v.out($v.hsv($p('0 0.25 0.5 0.75'), 1, $p.arrange([2, $p('1 0.5')], [1, $p('1')])))",
        "$v.out($v.hsv($sine('0.2hz').range(0, 1), 1, $v.shape($sine('1hz').range(0.1, 0.4))))",
        "$v.out($v.hsv($sine(['0.1hz', '0.13hz']).range(0, 0.5)))",
    ],
};

export const VIDEO_DOCS: VideoDoc[] = [
    {
        name: 'time',
        group: 'Generators',
        description:
            'Seconds since the patch started. It stands still while the patch is stopped and starts again from 0 when the patch is restarted. Use it as a phase to animate an oscillator.',
        params: [],
        examples: ['$v.out($v.colorize($v.osc(4, $v.time), 0, 0))'],
        declarations: ['readonly time: VideoSignal;'],
    },
    {
        name: 'ramp',
        group: 'Generators',
        description:
            'Scan ramp over the frame. Oscillators, shapes and the other modules turn ramps into patterns, so moving or turning a ramp moves or turns everything built from it.',
        params: [
            {
                name: 'axis',
                description:
                    "`'horizontal'` 0..1 across (default), `'vertical'` 0..1 up, `'radial'` distance from the center (0.5 at the top and bottom edges), `'angular'` angle around the center, 0..1 once around",
            },
            {
                name: 'config.zoom',
                description:
                    'Magnification about the center; above 1 enlarges the pattern (default 1)',
            },
            {
                name: 'config.rotate',
                description:
                    'Turns; positive turns the pattern clockwise (default 0)',
            },
            {
                name: 'config.shiftX',
                description:
                    'Horizontal move as a fraction of the width; positive moves right (default 0)',
            },
            {
                name: 'config.shiftY',
                description:
                    'Vertical move as a fraction of the height; positive moves up (default 0)',
            },
        ],
        examples: [
            "$v.out($v.colorize($v.ramp(), $v.ramp('vertical'), 0.5))",
            "$v.out($v.hsv($v.osc(6, $v.time, { input: $v.ramp('radial') })))",
            "$v.out($v.hsv($v.ramp('angular', { rotate: $v.time.$.osc(0.1) })))",
            "$v.out($v.colorize($v.osc(8, 0, { input: $v.ramp('horizontal', { rotate: 0.125, zoom: 2 }) }), 0.2, 0.5))",
        ],
        declarations: [
            "ramp(\n    axis?: 'horizontal' | 'vertical' | 'radial' | 'angular',\n    config?: {\n        zoom?: VideoValue;\n        rotate?: VideoValue;\n        shiftX?: VideoValue;\n        shiftY?: VideoValue;\n    },\n): VideoSignal;",
        ],
    },
    {
        name: 'osc',
        group: 'Generators',
        description:
            'A gray wave: `freq` cycles across the frame, offset by `phase` cycles. `config.input` runs it along another signal instead, `freq` cycles per unit of that signal, such as a radial ramp for rings or `$v.time` for a pulse that changes over time.',
        params: [
            {
                name: 'freq',
                description:
                    'Cycles across the frame, or per unit of `config.input` (default 10)',
            },
            { name: 'phase', description: 'Offset in cycles (default 0)' },
            {
                name: 'config.shape',
                description:
                    "`'sine'` (default), `'triangle'`, `'saw'` or `'square'`",
            },
            {
                name: 'config.input',
                description:
                    'What the wave runs along (default the horizontal ramp, 0 to 1 across the frame)',
            },
        ],
        examples: [
            '$v.out($v.colorize($v.osc(8), 0, 0))',
            "$v.out($v.colorize($v.osc(3, $v.time, { shape: 'saw' }), $v.osc(5, 0, { input: $v.ramp('vertical') }), 0.3))",
        ],
        declarations: [
            "osc(\n    freq?: VideoValue,\n    phase?: VideoValue,\n    config?: { shape?: 'sine' | 'triangle' | 'saw' | 'square'; input?: VideoValue },\n): VideoSignal;",
        ],
    },
    {
        name: 'fromAudio',
        group: 'Generators',
        description:
            'The recent audio-rate samples of an audio signal, laid along `position`: 0 is the oldest sample in the window and 1 the newest. Values are in volts, as the audio graph produces them, interpolated between samples. With the default horizontal ramp for `position` this is a scope: compare it with the vertical ramp to draw the wave. Unlike an audio signal used directly as an input, which is read once per frame, this shows every sample.',
        params: [
            {
                name: 'signal',
                description:
                    'An audio signal, such as an oscillator or an envelope, or a pattern from `$p(...)`; several channels are summed to one',
            },
            {
                name: 'position',
                description:
                    'Where along the window to read, 0 to 1 (default the horizontal ramp)',
            },
            {
                name: 'config.samples',
                description:
                    'Samples the window spans, 2 to 4096 (default 512; at 48 kHz, 48 samples are 1 ms)',
            },
            {
                name: 'config.trigger',
                description:
                    'Start the window at a rising zero crossing so a periodic wave holds still (default true)',
            },
        ],
        examples: [
            "$v.out($v.hsv(0.35, 1, $v.invert($v.comparator($v.diff($v.ramp('vertical'), $v.add(0.5, $v.mult($v.fromAudio($sine('110hz')), 0.08))), 0.01, 0.01))))",
            "$v.out($v.hsv($v.fromAudio($saw('55hz'), $v.ramp('radial'), { samples: 1024 }), 1, 1))",
        ],
        declarations: [
            'fromAudio(signal: ModuleOutput | Collection | CollectionWithRange | PatternSource, position?: VideoValue, config?: { samples?: number; trigger?: boolean }): VideoSignal;',
        ],
    },
    {
        name: 'noise',
        group: 'Generators',
        description:
            'Smooth gray noise between 0 and 1, simplex noise unless `config.type` names another algorithm, with about `scale` features up the frame. Its features keep their proportions in any window, and resizing the window shows more or less of the noise around the center rather than stretching it. `z` moves through the noise, which animates it when fed `$v.time`. With `config.mapX` or `config.mapY` it is read at those coordinates times `scale` instead.',
        params: [
            {
                name: 'scale',
                description:
                    'Features up the frame, or per unit of `config.mapX` and `config.mapY` (default 5)',
            },
            {
                name: 'z',
                description:
                    'Position through the noise, such as time (default 0)',
            },
            {
                name: 'config.type',
                description:
                    "`'simplex'` (default) smooth gradient noise without grid artifacts, `'perlin'` classic gradient noise, `'value'` blended random values, blockier and aligned to the grid, `'fbm'` four octaves of simplex noise for detail at several scales, like clouds, `'ridged'` four octaves of sharp creases, like mountain ridges or marble veins",
            },
            {
                name: 'config.mapX',
                description:
                    'Horizontal coordinate (default the horizontal ramp, 0 to 1 across the frame)',
            },
            {
                name: 'config.mapY',
                description:
                    'Vertical coordinate (default the vertical ramp, 0 to 1 up the frame)',
            },
        ],
        examples: [
            '$v.out($v.hsv($v.noise(6, $v.mult($v.time, 0.5))))',
            "$v.out($v.noise(3, $v.mult($v.time, 0.2), { type: 'fbm' }))",
            "$v.out($v.hsv(0.08, 0.6, $v.noise(4, $v.mult($v.time, 0.1), { type: 'ridged' })))",
            "$v.out($v.hsv(0.6, 1, $v.comparator($v.noise(1, $v.time, { mapX: $v.mult($v.ramp('radial'), 10), mapY: $v.mult($v.ramp('angular'), 6) }), 0.5, 0.05)))",
        ],
        declarations: [
            "noise(\n    scale?: VideoValue,\n    z?: VideoValue,\n    config?: { type?: 'simplex' | 'perlin' | 'value' | 'fbm' | 'ridged'; mapX?: VideoValue; mapY?: VideoValue },\n): VideoSignal;",
        ],
    },
    {
        name: 'voronoi',
        group: 'Generators',
        description:
            'Gray cellular noise: the distance to the nearest of a scatter of points, about `scale` cells up the frame, between 0 (on a point) and about 1. Its cells keep their proportions in any window, as for `$v.noise`. `z` moves the points, which animates the cells when fed `$v.time`. With `config.mapX` or `config.mapY` it is read at those coordinates times `scale` instead.',
        params: [
            {
                name: 'scale',
                description:
                    'Cells up the frame, or per unit of `config.mapX` and `config.mapY` (default 5)',
            },
            {
                name: 'z',
                description: 'Moves the points, such as time (default 0)',
            },
            {
                name: 'config.mapX',
                description:
                    'Horizontal coordinate (default the horizontal ramp, 0 to 1 across the frame)',
            },
            {
                name: 'config.mapY',
                description:
                    'Vertical coordinate (default the vertical ramp, 0 to 1 up the frame)',
            },
        ],
        examples: [
            "$v.out($v.hsv(0.55, 0.8, $v.voronoi(1, $v.mult($v.time, 2), { mapX: $v.mult($v.ramp(), 8), mapY: $v.mult($v.ramp('vertical'), 5) })))",
            '$v.voronoi(6, $v.time).$.invert().$.tint(0.1).out()',
        ],
        declarations: [
            'voronoi(scale?: VideoValue, z?: VideoValue, config?: { mapX?: VideoValue; mapY?: VideoValue }): VideoSignal;',
        ],
    },
    {
        name: 'polygon',
        group: 'Generators',
        description:
            'A gray mask: 1 inside a regular polygon centered on the frame with one point up, 0 outside, with a soft edge. With `config.mapX` and `config.mapY` it is centered where they are both 0.5, so adding to a ramp moves it.',
        params: [
            {
                name: 'sides',
                description: 'Number of sides, at least 3 (default 3)',
            },
            {
                name: 'size',
                description:
                    'Distance from the center to the middle of a side, as a fraction of the frame height (default 0.25)',
            },
            {
                name: 'softness',
                description:
                    'Width of the edge in the same units (default 0.01)',
            },
            {
                name: 'config.falloff',
                description:
                    "The curve the edge follows across `softness`: `'linear'` (default), `'smooth'` an S-curve that meets the inside and outside without a crease, `'smoother'` the same with flatter ends, `'in'` low for longer and rising sharply at the inside, `'out'` falling sharply at the outside",
            },
            {
                name: 'config.mapX',
                description:
                    'Horizontal coordinate (default the horizontal ramp, 0 to 1 across the frame)',
            },
            {
                name: 'config.mapY',
                description:
                    'Vertical coordinate (default the vertical ramp, 0 to 1 up the frame)',
            },
        ],
        examples: [
            '$v.out($v.hsv(0.12, 1, $v.polygon(5, 0.3, 0.02)))',
            '$v.polygon(6, 0.2).$.rotate($v.mult($v.time, 0.1)).$.tint(0.6).out()',
        ],
        declarations: [
            "polygon(\n    sides?: VideoValue,\n    size?: VideoValue,\n    softness?: VideoValue,\n    config?: {\n        falloff?: 'linear' | 'smooth' | 'smoother' | 'in' | 'out';\n        mapX?: VideoValue;\n        mapY?: VideoValue;\n    },\n): VideoSignal;",
        ],
    },
    {
        name: 'blur',
        group: 'Filters',
        description:
            "Averages `input` over a disk of `radius`, softening it. The input's whole sub-patch is read at sixteen nearby coordinates, so its cost is paid sixteen times: blur cheap things, or blur late. Adding a blurred copy back on top of the original is a glow.",
        params: [
            {
                name: 'radius',
                description:
                    'Radius of the disk as a fraction of the frame height (default 0.01)',
            },
        ],
        examples: [
            '$v.out($v.hsv($v.blur($v.polygon(5, 0.2, 0.001), 0.04)))',
            '$v.polygon(6, 0.18, 0.002).$.tint(0.12).pipe((c) => c.$.add(c.$.blur(0.04).$.mult(2))).out()',
        ],
        declarations: [
            'blur(input: VideoSignal, radius?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'bloom',
        group: 'Filters',
        description:
            'Adds a blurred copy of `input` back on top of it, so bright areas glow. It costs one blur.',
        params: [
            {
                name: 'radius',
                description:
                    'Radius of the blur as a fraction of the frame height (default 0.04)',
            },
            {
                name: 'amount',
                description:
                    'How strongly the blurred copy is added back (default 1)',
            },
        ],
        examples: [
            '$v.out($v.bloom($v.hsv(0.1, 1, $v.polygon(5, 0.15, 0.002)), 0.05, 2))',
            '$v.polygon(6, 0.15, 0.002).$.tint(0.55).$.bloom(0.06, 2.5).out()',
        ],
        declarations: [
            'bloom(input: VideoSignal, radius?: VideoValue, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'edges',
        group: 'Filters',
        description:
            'Brightness of the steepest change around each pixel (a Sobel filter): 0 on flat areas, rising along edges. It reads `input` at the eight neighbors one pixel away, so it sees the same pixel grid at any window size. It works on the brightness of `input`.',
        params: [
            {
                name: 'amount',
                description:
                    'Gain on the result, which is clipped to 0..1 (default 1)',
            },
        ],
        examples: ['$v.out($v.hsv(0.4, 1, $v.edges($v.noise(6, $v.time), 4)))'],
        declarations: [
            'edges(input: VideoSignal, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'image',
        group: 'Generators',
        description:
            'A picture from the workspace folder (`.png`, `.jpg`, `.gif`, `.webp`, `.bmp` or `.avif`) as a color. It is read at the coordinate being drawn, so warps, kaleidoscopes and feedback move it like any other pattern. Paths are relative to the workspace folder and cannot leave it. An `http://` or `https://` URL works too: the app fetches it itself, so the server needs no special headers.',
        params: [
            {
                name: 'path',
                description:
                    'File to draw, relative to the workspace folder, or an http(s) URL',
            },
            {
                name: 'config.fit',
                description:
                    "How a picture of another shape fills the frame: `'cover'` (default) fills it and crops the overflow, `'contain'` fits the whole picture and leaves black bars, `'stretch'` ignores the aspect ratio",
            },
        ],
        examples: [
            "$v.out($v.image('pictures/photo.png'))",
            "$v.out($v.image('https://example.com/photo.jpg'))",
            "$v.image('pictures/photo.png', { fit: 'contain' }).$.kaleid(6).$.hueShift($v.time.$.osc(0.05)).out()",
        ],
        declarations: [
            "image(path: string, config?: { fit?: 'cover' | 'contain' | 'stretch' }): VideoSignal;",
        ],
    },
    {
        name: 'video',
        group: 'Generators',
        description:
            "A recording from the workspace folder (`.mp4`, `.webm`, `.mov`, `.m4v` or `.ogv`) as a color, played in a loop. The picture is muted; the video's sound is its `.audio` property, an audio signal you route like any other, such as `$v.video('clips/loop.mp4').audio.out()`. The sound plays at the same `speed` between the same `loop` points, starting when the patch starts, and has no sound at speed 0. A file without sound has no `.audio`, and reading it is an error. Sound is read from AAC, MP3, Vorbis, FLAC and PCM tracks of up to ten minutes (Opus, common in `.webm`, is not read). For a video at a network address, use `$v.stream`, which has no sound. Like `$v.image`, it is read at the coordinate being drawn. It plays while the patch plays, holds its frame when the patch is stopped, and starts over from its loop start when the patch starts. Changing `speed` or `loop` and running the patch again keeps the video playing from where it is. Which formats play depends on the codecs the app's browser engine includes (H.264, HEVC, VP8/VP9 and AV1); `speed` and `loop` are fixed numbers, and video cannot play backwards.",
        params: [
            {
                name: 'path',
                description: 'File to play, relative to the workspace folder',
            },
            {
                name: 'config.fit',
                description:
                    "How a picture of another shape fills the frame: `'cover'` (default) fills it and crops the overflow, `'contain'` fits the whole picture and leaves black bars, `'stretch'` ignores the aspect ratio",
            },
            {
                name: 'config.speed',
                description:
                    'Playback rate from 0 to 16: 1 is normal speed, 0.5 half speed, 0 holds the current frame (default 1)',
            },
            {
                name: 'config.loop',
                description:
                    'Loop points in seconds as `[start, end]`; playback returns to `start` on reaching `end`. `end` defaults to the end of the file, so `[2]` loops the part from 2 seconds on',
            },
        ],
        examples: [
            "$v.out($v.video('clips/loop.mp4'))",
            "$v.video('clips/loop.mp4').$.mult($v.hsv($v.ramp(), 0.5, 1)).$.warp({ rotate: 0.02 }).out()",
            "$v.video('clips/loop.mp4', { speed: 0.5, loop: [1, 2.5] }).out()",
            "const clip = $v.video('clips/loop.mp4');\nclip.out();\nclip.audio.out();",
        ],
        declarations: [
            "video(\n    path: string,\n    config?: {\n        fit?: 'cover' | 'contain' | 'stretch';\n        speed?: number;\n        loop?: [start: number, end?: number];\n    },\n): VideoFile;",
        ],
    },
    {
        name: 'stream',
        group: 'Generators',
        description:
            'A video or live stream at an `http://` or `https://` address as a color, played as it arrives with its sound off. The address can be a video file, or an HLS stream when it ends in `.m3u8`; RTSP, RTMP and other protocols the app cannot play are refused. The app fetches the address itself, so the server needs no special headers. It is read at the coordinate being drawn, so warps, kaleidoscopes and feedback move it like any other pattern, and it plays while the patch plays.',
        params: [
            {
                name: 'url',
                description:
                    'An http(s) address of a video file or an .m3u8 HLS playlist',
            },
            {
                name: 'config.fit',
                description:
                    "How a picture of another shape fills the frame: `'cover'` (default) fills it and crops the overflow, `'contain'` fits the whole picture and leaves black bars, `'stretch'` ignores the aspect ratio",
            },
        ],
        examples: [
            "$v.out($v.stream('https://example.com/live/stream.m3u8'))",
            "$v.stream('https://example.com/clip.mp4', { fit: 'contain' }).$.hueShift($v.time.$.osc(0.05)).out()",
        ],
        declarations: [
            "stream(url: string, config?: { fit?: 'cover' | 'contain' | 'stretch' }): VideoSignal;",
        ],
    },
    {
        name: 'camera',
        group: 'Generators',
        description:
            'The live picture of a camera as a color. It is read at the coordinate being drawn, so warps, kaleidoscopes and feedback move it like any other pattern. The first time a patch uses it the system asks whether Operator may use the camera; if that is refused, allow it under System Settings, Privacy & Security, Camera, and restart Operator.',
        params: [
            {
                name: 'config.device',
                description:
                    "Part of the camera's name, matched without regard to case, such as `'FaceTime'`; the first camera by default",
            },
            {
                name: 'config.fit',
                description:
                    "How a picture of another shape fills the frame: `'cover'` (default) fills it and crops the overflow, `'contain'` fits the whole picture and leaves black bars, `'stretch'` ignores the aspect ratio",
            },
        ],
        examples: [
            '$v.out($v.camera())',
            "$v.camera({ fit: 'contain' }).$.kaleid(6).$.hueShift($v.time.$.osc(0.05)).out()",
        ],
        declarations: [
            "camera(config?: { device?: string; fit?: 'cover' | 'contain' | 'stretch' }): VideoSignal;",
        ],
    },
    {
        name: 'screen',
        group: 'Generators',
        description:
            'The live picture of a display as a color, read at the coordinate being drawn like `$v.camera`. The first time a patch uses it the system must allow Operator to record the screen, under System Settings, Privacy & Security, Screen Recording; after allowing it, restart Operator. Showing the display Operator itself is on draws the picture into itself, which makes a tunnel.',
        params: [
            {
                name: 'config.display',
                description: 'Which display, counting from 1 (default 1)',
            },
            {
                name: 'config.fit',
                description:
                    "How a picture of another shape fills the frame: `'cover'` (default), `'contain'` or `'stretch'`, as for `$v.camera`",
            },
        ],
        examples: [
            '$v.out($v.screen())',
            "$v.screen({ display: 2, fit: 'contain' }).$.hueShift($v.time.$.osc(0.05)).out()",
        ],
        declarations: [
            "screen(config?: { display?: number; fit?: 'cover' | 'contain' | 'stretch' }): VideoSignal;",
        ],
    },
    {
        name: 'shape',
        group: 'Generators',
        description:
            'A gray mask: 1 inside a shape centered on the frame and 0 outside, with a soft edge. With `config.mapX` and `config.mapY` it is centered where they are both 0.5, so adding to a ramp moves it. Circles stay round at any window aspect.',
        params: [
            {
                name: 'size',
                description:
                    'Half-extent as a fraction of frame height (default 0.25)',
            },
            {
                name: 'softness',
                description:
                    'Width of the edge in the same units (default 0.01)',
            },
            {
                name: 'config.shape',
                description: "`'circle'` (default), `'box'` or `'diamond'`",
            },
            {
                name: 'config.falloff',
                description:
                    "The curve the edge follows across `softness`: `'linear'` (default), `'smooth'` an S-curve that meets the inside and outside without a crease, `'smoother'` the same with flatter ends, `'in'` low for longer and rising sharply at the inside, `'out'` falling sharply at the outside",
            },
            {
                name: 'config.mapX',
                description:
                    'Horizontal coordinate (default the horizontal ramp, 0 to 1 across the frame)',
            },
            {
                name: 'config.mapY',
                description:
                    'Vertical coordinate (default the vertical ramp, 0 to 1 up the frame)',
            },
        ],
        examples: [
            '$v.out($v.hsv(0.6, 1, $v.shape(0.3, 0.05)))',
            '$v.out($v.hsv(0.1, 1, $v.shape(0.15, 0.01, { mapX: $v.add($v.ramp(), $v.time.$.osc(0.25).range(-0.3, 0.3)) })))',
            "$v.out($v.hsv($v.time, 1, $v.shape(0.2, 0, { shape: 'diamond' })))",
            "$v.out($v.shape(0.3, 0.3, { falloff: 'smooth' }))",
        ],
        declarations: [
            "shape(\n    size?: VideoValue,\n    softness?: VideoValue,\n    config?: {\n        shape?: 'circle' | 'box' | 'diamond';\n        falloff?: 'linear' | 'smooth' | 'smoother' | 'in' | 'out';\n        mapX?: VideoValue;\n        mapY?: VideoValue;\n    },\n): VideoSignal;",
        ],
    },
    {
        name: 'colorize',
        group: 'Color',
        description:
            'A color from red, green, blue and alpha levels, each clipped to 0..1.',
        params: [{ name: 'a', description: 'Alpha (default 1, opaque)' }],
        examples: ["$v.out($v.colorize($v.ramp(), $v.ramp('vertical'), 1))"],
        declarations: [
            'colorize(r: VideoValue, g: VideoValue, b: VideoValue, a?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'hsv',
        group: 'Color',
        description: 'An opaque color from hue, saturation and value.',
        params: [
            {
                name: 'hue',
                description:
                    'Position round the color wheel, wrapping every 1: 0 red, 1/3 green, 2/3 blue',
            },
            {
                name: 'saturation',
                description:
                    'From 0 gray to 1 the pure hue, clipped to 0..1 (default 1)',
            },
            {
                name: 'value',
                description:
                    'Brightness, from 0 black to 1 full, clipped to 0..1 (default 1)',
            },
        ],
        examples: [
            '$v.out($v.hsv($v.ramp(), 1, 1))',
            "$v.out($v.hsv($v.osc(2, $v.time), 0.8, $v.ramp('vertical')))",
        ],
        declarations: [
            'hsv(hue: VideoValue, saturation?: VideoValue, value?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'procAmp',
        group: 'Color',
        description:
            'Saturation, then gain and bias, clipped to the displayable range.',
        params: [
            {
                name: 'input',
                description: 'The signal to adjust',
            },
            { name: 'gain', description: 'Multiplier (default 1)' },
            {
                name: 'bias',
                description: 'Offset added after gain (default 0)',
            },
            {
                name: 'saturation',
                description: '0 is gray, 1 is unchanged (default 1)',
            },
        ],
        examples: ['$v.out($v.procAmp($v.hsv($v.ramp()), 1.5, -0.2, 0.5))'],
        declarations: [
            'procAmp(\n    input: VideoValue,\n    gain?: VideoValue,\n    bias?: VideoValue,\n    saturation?: VideoValue,\n): VideoSignal;',
        ],
    },
    {
        name: 'add',
        group: 'Math',
        description:
            'Sum of each channel, clipped to 0..1. The alpha is the larger of the two.',
        params: [],
        examples: [
            "$v.out($v.hsv($v.add($v.ramp(), $v.osc(3, 0, { input: $v.ramp('vertical') }))))",
        ],
        declarations: ['add(a: VideoValue, b: VideoValue): VideoSignal;'],
    },
    {
        name: 'mult',
        group: 'Math',
        description:
            'Product of each channel, alpha included. Multiplying by a gray darkens.',
        params: [],
        examples: ["$v.out($v.mult($v.hsv($v.ramp()), $v.ramp('vertical')))"],
        declarations: ['mult(a: VideoValue, b: VideoValue): VideoSignal;'],
    },
    {
        name: 'diff',
        group: 'Math',
        description:
            'Absolute difference of each channel. The alpha is the larger of the two.',
        params: [],
        examples: ["$v.out($v.hsv($v.diff($v.ramp(), $v.ramp('vertical'))))"],
        declarations: ['diff(a: VideoValue, b: VideoValue): VideoSignal;'],
    },
    {
        name: 'max',
        group: 'Math',
        description:
            'The larger of each channel. With shapes this is their union.',
        params: [],
        examples: [
            '$v.out($v.hsv(0.1, 1, $v.max($v.shape(0.2), $v.shape(0.2, 0.01, { mapX: $v.add($v.ramp(), 0.2) }))))',
        ],
        declarations: ['max(a: VideoValue, b: VideoValue): VideoSignal;'],
    },
    {
        name: 'min',
        group: 'Math',
        description:
            'The smaller of each channel. With shapes this is their intersection.',
        params: [],
        examples: [
            '$v.out($v.hsv(0.5, 1, $v.min($v.shape(0.3), $v.shape(0.3, 0.01, { mapX: $v.add($v.ramp(), 0.2) }))))',
        ],
        declarations: ['min(a: VideoValue, b: VideoValue): VideoSignal;'],
    },
    {
        name: 'invert',
        group: 'Math',
        description: 'Complement, 1 minus the input.',
        params: [],
        examples: ['$v.out($v.invert($v.hsv($v.ramp())))'],
        declarations: ['invert(input: VideoValue): VideoSignal;'],
    },
    {
        name: 'mix',
        group: 'Math',
        description: 'Crossfade from `a` (amount 0) to `b` (amount 1).',
        params: [
            {
                name: 'amount',
                description: 'Mix position, clipped to 0..1 (default 0.5)',
            },
        ],
        examples: [
            "$v.out($v.mix($v.hsv($v.ramp()), $v.hsv($v.ramp('vertical')), $v.time.$.osc(0.25)))",
        ],
        declarations: [
            'mix(a: VideoValue, b: VideoValue, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'range',
        group: 'Shaping',
        description:
            "Maps each channel's 0 to 1 onto `min` to `max`, as `.range` does for an audio signal; the alpha is kept. Use it to give an input that has its own units, such as a zoom factor, a frequency or a count, the span you want. It is also called on the signal itself, as `signal.range(min, max)`.",
        params: [
            { name: 'min', description: 'The value 0 becomes (default 0)' },
            { name: 'max', description: 'The value 1 becomes (default 1)' },
        ],
        examples: [
            "$v.out($v.hsv($v.ramp('angular', { zoom: $v.range($v.time.$.osc(0.1), 1, 3) })))",
            '$v.out($v.hsv($v.ramp().range(0, 0.5)))',
        ],
        declarations: [
            'range(input: VideoValue, min?: VideoValue, max?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'wrap',
        group: 'Shaping',
        description:
            'Multiplies by `gain`, then keeps the fractional part: a sawtooth of the input that repeats `gain` times as the input goes from 0 to 1.',
        params: [],
        examples: ['$v.out($v.hsv($v.wrap($v.ramp(), 4)))'],
        declarations: [
            'wrap(input: VideoValue, gain?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'fold',
        group: 'Shaping',
        description:
            'Multiplies by `gain`, then reflects whatever passes 1 back down: a triangle of the input that matches it where it stays under 1 / gain.',
        params: [],
        examples: ['$v.out($v.hsv($v.fold($v.ramp(), 3)))'],
        declarations: [
            'fold(input: VideoValue, gain?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'comparator',
        group: 'Shaping',
        description:
            'Threshold of each channel: 0 below `threshold`, 1 above, with a linear ramp `softness` wide centered on it (a hard edge when 0). `config.invert` swaps the sides: 1 below and 0 above.',
        params: [
            {
                name: 'config.invert',
                description:
                    'Give 1 below the threshold and 0 above (default false)',
            },
        ],
        examples: [
            '$v.out($v.hsv(0.1, 1, $v.comparator($v.osc(4), 0.5, 0.1)))',
            '$v.out($v.hsv(0.6, 1, $v.comparator($v.ramp(), 0.3, 0, { invert: true })))',
        ],
        declarations: [
            'comparator(\n    input: VideoValue,\n    threshold?: VideoValue,\n    softness?: VideoValue,\n    config?: { invert?: boolean },\n): VideoSignal;',
        ],
    },
    {
        name: 'posterize',
        group: 'Shaping',
        description:
            'Quantizes to `levels` evenly spaced values between 0 and 1.',
        params: [
            {
                name: 'levels',
                description: 'Number of steps, at least 2 (default 4)',
            },
        ],
        examples: ['$v.out($v.hsv($v.posterize($v.ramp(), 5)))'],
        declarations: [
            'posterize(input: VideoValue, levels?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'key',
        group: 'Compositing',
        description: 'Shows `fg` where `mask` is 1 and `bg` where it is 0.',
        params: [],
        examples: [
            '$v.out($v.key($v.hsv(0.9), $v.hsv($v.ramp(), 1, 0.5), $v.shape(0.3)))',
        ],
        declarations: [
            'key(fg: VideoValue, bg: VideoValue, mask: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'layer',
        group: 'Compositing',
        description:
            'Lays `top` over `base` by the alpha of `top`: where `top` is opaque it covers `base`, and where it is transparent `base` shows through. The alphas add. `luma` and `mask` clear the color of what they key out, so a keyed-out part of `base` adds no color under a translucent `top`.',
        params: [
            { name: 'base', description: 'The signal underneath' },
            { name: 'top', description: 'The signal laid over it' },
        ],
        examples: [
            '$v.out($v.layer($v.osc(8).$.hsv(), $v.noise(4, $v.time).$.luma(0.5, 0.05)))',
            '$v.osc(20, $v.time).$.tint(0.6).$.layer($v.hsv(0.1, 1, $v.shape(0.2)).$.mask($v.shape(0.2))).out()',
        ],
        declarations: [
            'layer(base: VideoValue, top: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'mask',
        group: 'Compositing',
        description:
            '`input` seen through `mask`: its color and alpha are both scaled by the brightness of `mask`, as in Hydra, so it is opaque where the mask is white and transparent and black where it is black. What is masked out adds nothing to `add`, `diff`, `mix`, `blur` and the other combinations. Lay the result over another signal with `layer`.',
        params: [
            { name: 'mask', description: 'Where `input` shows, by brightness' },
        ],
        examples: [
            '$v.out($v.layer($v.hsv(0.6), $v.osc(10).$.hsv().$.mask($v.shape(0.3, 0.05))))',
        ],
        declarations: [
            'mask(input: VideoValue, mask: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'luma',
        group: 'Compositing',
        description:
            'Keys `input` by its own brightness: parts at least `threshold` bright stay opaque, parts darker than `threshold - tolerance` turn transparent, and the alpha rises smoothly in between. The color is scaled by the same key, as in Hydra, so what is keyed out is black as well as transparent and adds nothing to `add`, `diff`, `mix`, `blur` and the other combinations; `luma(1)` keeps only what is pure white. `config.invert` keeps the dark side instead, so `luma(0, 0.05, { invert: true })` keeps only black. Lay the result over another signal with `layer`.',
        params: [
            {
                name: 'threshold',
                description:
                    'Brightness from which the signal is fully opaque (default 0.5)',
            },
            {
                name: 'tolerance',
                description:
                    'Width of the band below `threshold` over which the alpha rises from 0 (default 0.1)',
            },
            {
                name: 'config.invert',
                description:
                    'Keep the dark side instead: opaque up to `threshold`, transparent from `threshold + tolerance` (default false)',
            },
        ],
        examples: [
            '$v.out($v.layer($v.hsv(0.7, 0.6, 0.4), $v.voronoi(6, $v.time).$.luma(0.4, 0.02)))',
            '$v.out($v.layer($v.osc(8).$.hsv(), $v.noise(4, $v.time).$.luma(0.4, 0.05, { invert: true })))',
        ],
        declarations: [
            'luma(input: VideoValue, threshold?: VideoValue, tolerance?: VideoValue, config?: { invert?: boolean }): VideoSignal;',
        ],
    },
    {
        name: 'feedback',
        group: 'Memory',
        description:
            "Feeds a frame back into itself. `update` receives the previous frame's result as `prev`, resampled through the transform in `config`, and returns this frame's color; `feedback` returns that color. The first frame's `prev` is transparent black. Re-running a patch keeps the loops' pictures, so edits take effect without wiping the trails.",
        params: [
            { name: 'update', description: 'Builds this frame from `prev`' },
            {
                name: 'config.zoom',
                description:
                    'Magnification of the previous frame about the center per frame; above 1 zooms in (default 1)',
            },
            {
                name: 'config.rotate',
                description:
                    'Turns per frame; positive turns the picture clockwise (default 0)',
            },
            {
                name: 'config.shiftX',
                description:
                    'Horizontal move per frame as a fraction of the width; positive moves right (default 0)',
            },
            {
                name: 'config.shiftY',
                description:
                    'Vertical move per frame as a fraction of the height; positive moves up (default 0)',
            },
            {
                name: 'config.edge',
                description:
                    "What lies beyond the frame border: `'clamp'` (default), `'repeat'` or `'mirror'`",
            },
        ],
        examples: [
            '$v.out($v.feedback((prev) => $v.mix($v.hsv($v.time, 1, $v.shape(0.1)), prev, 0.9), { zoom: 1.02, rotate: 0.004 }))',
            '$v.out($v.feedback((prev) => $v.add($v.hsv($v.time.$.osc(0.2), 1, $v.shape(0.05, 0.01, { mapX: $v.add($v.ramp(), -0.2) })), $v.mult(prev, 0.96)), { rotate: 0.01 }))',
        ],
        declarations: [
            "feedback(\n    update: (prev: VideoSignal) => VideoSignal,\n    config?: {\n        zoom?: VideoValue;\n        rotate?: VideoValue;\n        shiftX?: VideoValue;\n        shiftY?: VideoValue;\n        edge?: 'clamp' | 'repeat' | 'mirror';\n    },\n): VideoSignal;",
        ],
    },
    {
        name: 'frameDelay',
        group: 'Memory',
        description:
            '`input` as it was `frames` frames ago; until that many frames have passed it is transparent black. Add it back to the live picture for echoes and trails. Each frame held back takes one of the seven frame buffers that feedback loops and `$v.buffer` also use, so a long delay leaves fewer for them.',
        params: [
            {
                name: 'frames',
                description:
                    'Whole number of frames to hold back, 1 to 7 (default 1)',
            },
        ],
        examples: [
            'const dot = $v.hsv(0.3, 1, $v.shape(0.12, 0.01, { mapX: $v.add($v.ramp(), $v.time.$.osc(0.5)) })); $v.out($v.add(dot, $v.frameDelay(dot, 6).$.mult(0.5)))',
        ],
        declarations: [
            'frameDelay(input: VideoValue, frames?: number): VideoSignal;',
        ],
    },
    {
        name: 'preview',
        group: 'Output',
        description:
            'Shows a signal in the editor, in a panel under this call, and returns it unchanged so a preview can sit inside an expression. The panel is drawn from the same frame the picture shows, feedback included.',
        params: [
            {
                name: 'config.view',
                description:
                    "How the panel draws the signal: `'image'` (default) shows it as a picture, with transparent parts over a checkerboard, `'waveform'` plots brightness against horizontal position like a waveform monitor, `'vectorscope'` plots every pixel's color by hue and saturation",
            },
        ],
        examples: [
            '$v.out($v.hsv($v.preview($v.osc(4))))',
            "$v.out($v.preview($v.hsv($v.ramp('radial')), { view: 'waveform' }))",
            "$v.out($v.preview($v.hsv($v.ramp('angular')), { view: 'vectorscope' }))",
        ],
        declarations: [
            "preview(signal: VideoSignal, config?: { view?: 'image' | 'waveform' | 'vectorscope' }): VideoSignal;",
        ],
    },
    {
        name: 'toCV',
        group: 'Output',
        description:
            'Averages a region of a signal each frame into an audio control signal between 0 and 1, so the picture can modulate the sound: the brightness of the region as the picture shows it, over black. The value is read from the picture about 30 times a second. Scale the result with `.range(min, max)` like any ranged signal.',
        params: [
            {
                name: 'config.x',
                description:
                    'Center of the region as a fraction of the frame width (default 0.5)',
            },
            {
                name: 'config.y',
                description:
                    'Center of the region as a fraction of the frame height, 0 at the bottom (default 0.5)',
            },
            {
                name: 'config.size',
                description:
                    "Half the region's width and height as a fraction of the frame; 0.5, the default, is the whole frame",
            },
        ],
        examples: [
            '$sine($v.toCV($v.shape(0.3), { size: 0.1 }).range(110, 440)).out()',
        ],
        declarations: [
            'toCV(signal: VideoSignal, config?: { x?: number; y?: number; size?: number }): CollectionWithRange;',
        ],
    },
    {
        name: 'warp',
        group: 'Warping',
        description:
            'Zooms about the center, turns and shifts everything `input` draws: not just its output but the whole sub-patch behind it, which is evaluated again at the moved coordinates.',
        params: [
            {
                name: 'config.zoom',
                description:
                    'Magnification about the center; above 1 enlarges (default 1)',
            },
            {
                name: 'config.rotate',
                description: 'Turns; positive turns clockwise (default 0)',
            },
            {
                name: 'config.shiftX',
                description:
                    'Horizontal move as a fraction of the width; positive moves right (default 0)',
            },
            {
                name: 'config.shiftY',
                description:
                    'Vertical move as a fraction of the height; positive moves up (default 0)',
            },
        ],
        examples: [
            '$v.out($v.hsv($v.warp($v.noise(4), { rotate: $v.mult($v.time, 0.05), zoom: 2 })))',
            '$v.out($v.warp($v.hsv($v.shape(0.15)), { shiftX: $v.time.$.osc(0.25), rotate: 0.1 }))',
        ],
        declarations: [
            'warp(input: VideoSignal, config?: { zoom?: VideoValue; rotate?: VideoValue; shiftX?: VideoValue; shiftY?: VideoValue }): VideoSignal;',
        ],
    },
    {
        name: 'displace',
        group: 'Warping',
        description:
            'Reads `input` at positions pushed by `dx` and `dy`, so a moving or noisy push wobbles whatever `input` draws. 0.5 means no push in that direction.',
        params: [
            {
                name: 'dx',
                description:
                    'Horizontal push; below 0.5 pulls left, above pushes right',
            },
            {
                name: 'dy',
                description: 'Vertical push, the same way (default 0.5)',
            },
            {
                name: 'amount',
                description:
                    'Largest push as a fraction of the frame (default 0.1)',
            },
        ],
        examples: [
            '$v.out($v.hsv($v.displace($v.osc(8), $v.noise(3, $v.time), 0.5, 0.2)))',
        ],
        declarations: [
            'displace(input: VideoSignal, dx: VideoValue, dy?: VideoValue, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulate',
        group: 'Warping',
        description:
            "Pushes `input` around by another signal: the modulator's red channel moves it across and its green up, `amount` of the frame for each unit of the channel. A black modulator leaves `input` in place, and a gray one moves it diagonally.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that pushes `input`',
            },
            {
                name: 'amount',
                description:
                    'Push as a fraction of the frame for each unit of the channel (default 0.1)',
            },
        ],
        examples: [
            '$v.out($v.hsv($v.modulate($v.osc(10), $v.noise(3, $v.time), 0.3)))',
            "$v.out($v.modulate($v.hsv($v.ramp('radial')), $v.hsv($v.noise(4, 0)), 0.2))",
        ],
        declarations: [
            'modulate(input: VideoSignal, modulator: VideoSignal, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateScale',
        group: 'Warping',
        description:
            "Scales `input` about the center by `offset + multiple * channel`, the modulator's red channel setting the horizontal factor and its green the vertical. Where the modulator is 0 the factor is `offset`, and where it is full it is `offset + multiple`.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            {
                name: 'multiple',
                description:
                    'How much the modulator adds to the scale factor (default 1)',
            },
            {
                name: 'offset',
                description:
                    'The scale factor where the modulator is 0 (default 1)',
            },
        ],
        examples: [
            "$v.out($v.modulateScale($v.hsv($v.osc(10, 0, { input: $v.ramp('radial') })), $v.noise(3, $v.time), 1.5, 1))",
        ],
        declarations: [
            'modulateScale(input: VideoSignal, modulator: VideoSignal, multiple?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateRotate',
        group: 'Warping',
        description:
            'Turns `input` about the center by `offset + multiple * red`, so the brighter parts of the modulator turn the picture further. Angles are in turns: 1 is a full turn.',
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            {
                name: 'multiple',
                description: 'Turn at a full modulator, in turns (default 1)',
            },
            {
                name: 'offset',
                description: 'Turn where the modulator is 0 (default 0)',
            },
        ],
        examples: [
            "$v.out($v.modulateRotate($v.hsv($v.osc(8)), $v.osc(3, $v.time, { input: $v.ramp('radial') }), 0.5))",
        ],
        declarations: [
            'modulateRotate(input: VideoSignal, modulator: VideoSignal, multiple?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulatePixelate',
        group: 'Warping',
        description:
            'Holds `input` constant over a grid of `offset + multiple * channel` cells, red setting the cells across and green the cells up, so the pixels grow large where the modulator is dark.',
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            {
                name: 'multiple',
                description: 'Cells the modulator adds when full (default 10)',
            },
            {
                name: 'offset',
                description: 'Cells where the modulator is 0 (default 3)',
            },
        ],
        examples: [
            '$v.out($v.modulatePixelate($v.hsv($v.noise(4, $v.time)), $v.ramp(), 30, 4))',
        ],
        declarations: [
            'modulatePixelate(input: VideoSignal, modulator: VideoSignal, multiple?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateKaleid',
        group: 'Warping',
        description:
            "Mirrors `input` around the center into `sides` wedges, with the distance from the center pushed out by the modulator's red channel, so the kaleidoscope swells where the modulator is bright.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            { name: 'sides', description: 'Number of wedges (default 4)' },
        ],
        examples: [
            "$v.out($v.modulateKaleid($v.hsv($v.osc(6, $v.time)), $v.ramp('radial'), 6))",
        ],
        declarations: [
            'modulateKaleid(input: VideoSignal, modulator: VideoSignal, sides?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateHue',
        group: 'Warping',
        description:
            "Pushes `input` by the differences between the modulator's color channels, green minus red across and blue minus green up, by `amount` pixels where a difference is full scale. A gray modulator has the same value in every channel, so it has no differences and does not move `input`.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            {
                name: 'amount',
                description:
                    'Pixels of push for a channel difference of full scale (default 1)',
            },
        ],
        examples: [
            '$v.out($v.modulateHue($v.hsv($v.osc(8)), $v.hsv($v.noise(3, $v.time)), 120))',
        ],
        declarations: [
            'modulateHue(input: VideoSignal, modulator: VideoSignal, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateRepeat',
        group: 'Warping',
        description:
            "Tiles `input` `repeatX` by `repeatY` times with alternate rows and columns offset, the offsets growing with the modulator's red channel for x and green for y. 1 is a whole tile.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            { name: 'repeatX', description: 'Tiles across (default 3)' },
            { name: 'repeatY', description: 'Tiles up (default 3)' },
            {
                name: 'offsetX',
                description:
                    'Shift of alternate rows at a full red channel, with 1 a whole tile (default 0.5)',
            },
            {
                name: 'offsetY',
                description:
                    'Shift of alternate columns at a full green channel (default 0.5)',
            },
        ],
        examples: [
            '$v.out($v.modulateRepeat($v.hsv(0.6, 1, $v.shape(0.3)), $v.noise(3, $v.time), 4, 4, 0.5, 0.5))',
        ],
        declarations: [
            'modulateRepeat(input: VideoSignal, modulator: VideoSignal, repeatX?: VideoValue, repeatY?: VideoValue, offsetX?: VideoValue, offsetY?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateRepeatX',
        group: 'Warping',
        description:
            "Tiles `input` `reps` times across, shifting alternate columns up by `offset` times the modulator's red channel.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            { name: 'reps', description: 'Tiles across (default 3)' },
            {
                name: 'offset',
                description:
                    'Shift at a full red channel, with 1 a whole tile (default 0.5)',
            },
        ],
        examples: [
            '$v.out($v.modulateRepeatX($v.hsv(0.6, 1, $v.shape(0.3)), $v.noise(3, $v.time), 4))',
        ],
        declarations: [
            'modulateRepeatX(input: VideoSignal, modulator: VideoSignal, reps?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateRepeatY',
        group: 'Warping',
        description:
            "Tiles `input` `reps` times up, shifting alternate rows right by `offset` times the modulator's red channel.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            { name: 'reps', description: 'Tiles up (default 3)' },
            {
                name: 'offset',
                description:
                    'Shift at a full red channel, with 1 a whole tile (default 0.5)',
            },
        ],
        examples: [
            '$v.out($v.modulateRepeatY($v.hsv(0.6, 1, $v.shape(0.3)), $v.noise(3, $v.time), 4))',
        ],
        declarations: [
            'modulateRepeatY(input: VideoSignal, modulator: VideoSignal, reps?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateScrollX',
        group: 'Warping',
        description:
            "Scrolls `input` across by the modulator's red channel times `scroll`, plus `speed` of the frame every second, wrapping round the edges.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            {
                name: 'scroll',
                description:
                    'Scroll at a full red channel, with 1 the whole width (default 0.5)',
            },
            {
                name: 'speed',
                description:
                    'Scroll per second, with 1 the whole width (default 0)',
            },
        ],
        examples: [
            "$v.out($v.modulateScrollX($v.hsv($v.osc(8)), $v.ramp('vertical'), 0.5, 0.1))",
        ],
        declarations: [
            'modulateScrollX(input: VideoSignal, modulator: VideoSignal, scroll?: VideoValue, speed?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateScrollY',
        group: 'Warping',
        description:
            "Scrolls `input` up by the modulator's red channel times `scroll`, plus `speed` of the frame every second, wrapping round the edges.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that moves `input` by its channels',
            },
            {
                name: 'scroll',
                description:
                    'Scroll at a full red channel, with 1 the whole height (default 0.5)',
            },
            {
                name: 'speed',
                description:
                    'Scroll per second, with 1 the whole height (default 0)',
            },
        ],
        examples: [
            "$v.out($v.modulateScrollY($v.hsv($v.osc(8, 0, { input: $v.ramp('vertical') })), $v.ramp(), 0.5, 0.1))",
        ],
        declarations: [
            'modulateScrollY(input: VideoSignal, modulator: VideoSignal, scroll?: VideoValue, speed?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'kaleid',
        group: 'Warping',
        description:
            'Mirrors `input` around the center into `sides` wedges, like a kaleidoscope.',
        params: [
            { name: 'sides', description: 'Number of wedges (default 4)' },
        ],
        examples: [
            '$v.out($v.kaleid($v.hsv($v.osc(3, $v.time)), 6))',
            '$v.out($v.hsv($v.kaleid($v.noise(5, $v.time), 8)))',
        ],
        declarations: [
            'kaleid(input: VideoSignal, sides?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'pixelate',
        group: 'Warping',
        description:
            'Holds `input` constant across a grid of `x` by `y` cells, giving it large square pixels.',
        params: [
            { name: 'x', description: 'Cells across (default 20)' },
            { name: 'y', description: 'Cells up (default the same as x)' },
        ],
        examples: ['$v.out($v.hsv($v.pixelate($v.noise(4, $v.time), 24, 14)))'],
        declarations: [
            'pixelate(input: VideoSignal, x?: VideoValue, y?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'repeat',
        group: 'Warping',
        description: 'Tiles `input` `x` by `y` times across the frame.',
        params: [
            { name: 'x', description: 'Tiles across (default 3)' },
            { name: 'y', description: 'Tiles up (default the same as x)' },
        ],
        examples: ['$v.out($v.repeat($v.hsv(0.6, 1, $v.shape(0.3)), 4, 3))'],
        declarations: [
            'repeat(input: VideoSignal, x?: VideoValue, y?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'hueShift',
        group: 'Color',
        description:
            'Turns every hue of `input` by `amount` of a full circle, keeping its saturation, brightness and alpha.',
        params: [
            {
                name: 'amount',
                description:
                    'Fraction of the hue circle to turn by; 0.5 is the opposite hue (default 0.5)',
            },
        ],
        examples: [
            '$v.out($v.hueShift($v.hsv($v.ramp()), $v.time.$.osc(0.1)))',
        ],
        declarations: [
            'hueShift(input: VideoValue, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'contrast',
        group: 'Color',
        description:
            "Scales each channel's distance from mid-gray by `amount`: above 1 pushes colors apart, below 1 pulls them toward gray.",
        params: [
            { name: 'amount', description: 'Contrast factor (default 1.6)' },
        ],
        examples: [
            "$v.out($v.contrast($v.hsv($v.ramp(), 0.6, $v.ramp('vertical')), 3))",
        ],
        declarations: [
            'contrast(input: VideoValue, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'scanlines',
        group: 'Post',
        description:
            "Darkens evenly spaced horizontal lines, as a CRT's raster does.",
        params: [
            {
                name: 'count',
                description: 'Lines across the frame height (default 240)',
            },
            {
                name: 'strength',
                description:
                    'How dark each line gets at its darkest, 0 to 1 (default 0.4)',
            },
        ],
        examples: ['$v.out($v.scanlines($v.hsv($v.ramp()), 120, 0.5))'],
        declarations: [
            'scanlines(input: VideoValue, count?: VideoValue, strength?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'vignette',
        group: 'Post',
        description:
            'Darkens toward the corners of the frame: nothing within `radius` of the center, then up to `strength` by the edge.',
        params: [
            {
                name: 'strength',
                description: 'How dark the corners get, 0 to 1 (default 0.6)',
            },
            {
                name: 'radius',
                description:
                    'Distance from the center, as a fraction of the frame height, where the darkening starts (default 0.3)',
            },
        ],
        examples: ['$v.out($v.vignette($v.hsv($v.ramp()), 0.8, 0.2))'],
        declarations: [
            'vignette(input: VideoValue, strength?: VideoValue, radius?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'grain',
        group: 'Post',
        description:
            'Adds film grain: a fresh random value for every pixel, redrawn sixty times a second, of up to `amount` in either direction.',
        params: [
            {
                name: 'amount',
                description:
                    'Largest change to a channel, 0 to 1 (default 0.1)',
            },
        ],
        examples: [
            '$v.out($v.grain($v.hsv($v.ramp(), 0.6, 0.6), 0.15))',
            '$v.hsv($v.ramp()).$.scanlines(180, 0.35).$.vignette(0.7).$.grain(0.08).out()',
        ],
        declarations: [
            'grain(input: VideoValue, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'channel',
        group: 'Color',
        description:
            "One channel of a signal as an opaque gray: `'r'`, `'g'`, `'b'`, `'a'` for the alpha, or `'luma'` for brightness (the default). The channels are also every signal's `.r`, `.g`, `.b` and `.a` properties.",
        params: [{ name: 'which', description: 'Which channel to take' }],
        examples: [
            "$v.out($v.colorize($v.channel($v.hsv($v.ramp()), 'r'), 0, 0))",
        ],
        declarations: [
            "channel(input: VideoValue, which?: 'r' | 'g' | 'b' | 'a' | 'luma'): VideoSignal;",
        ],
    },
    {
        name: 'swiz',
        group: 'Color',
        description:
            "Reorders the channels of a signal. Each letter of `pattern`, one of `r`, `g`, `b` and `a`, names the channel of `input` that fills red, green and blue in turn: `'gbr'` makes red from green, green from blue and blue from red, `'rrr'` is the red channel as gray, and `'aaa'` shows the alpha. The alpha is kept.",
        params: [
            {
                name: 'pattern',
                description:
                    "Three of `r`, `g`, `b` and `a`, such as `'rrr'`, `'gbr'` or `'aaa'`",
            },
        ],
        examples: [
            "$v.out($v.swiz($v.hsv($v.ramp()), 'gbr'))",
            "$v.hsv($v.ramp()).$.swiz('bgr').out()",
        ],
        declarations: [
            'swiz(input: VideoValue, pattern: string): VideoSignal;',
        ],
    },
    {
        name: 'buffer',
        group: 'Memory',
        description:
            "A frame store that persists from one frame to the next. `write` stores a color; `read` returns what the buffer held on the previous frame, resampled through a zoom, rotation, shift and edge mode (see `$v.feedback`). Any number of signals can read a buffer, and buffers can read each other, which `$v.feedback`'s single loop cannot express. A buffer that is read must be written, and can be written once. A patch can use seven buffers and feedback loops together.",
        params: [],
        examples: [
            'const trail = $v.buffer(); $v.osc(8, $v.time).$.hsv().$.mult(0.2).$.add(trail.read({ zoom: 1.01, rotate: 0.002 }).$.mult(0.96)).write(trail).out()',
            'const a = $v.buffer(); const b = $v.buffer(); a.write($v.hsv($v.time, 1, $v.shape(0.08)).$.add(b.read({ rotate: 0.01 }).$.mult(0.95))); b.write(a.read({ zoom: 1.03 }).$.mult(0.9)); $v.out(a.read())',
        ],
        declarations: ['buffer(): VideoBuffer;'],
    },
    {
        name: 'out',
        group: 'Output',
        description:
            'Shows a signal behind the code, and in the performance window when it is open, laid over black by its alpha. Last call wins. It returns its input unchanged, so it can sit in the middle of a chain.',
        params: [],
        examples: ['$v.out($v.colorize(1, 0, 0))'],
        declarations: ['out<T extends VideoSignal>(input: T): T;'],
    },
];

/** One method a video signal can be chained with. */
export interface VideoChainDoc {
    name: string;
    description: string;
    declarations: string[];
    /** A method of the signal itself that ends or taps a chain, not a member of `.$`. */
    direct?: boolean;
}

export const VIDEO_CHAIN_INTRO =
    'Video signals chain the way audio signals do. `.$` is a namespace of the `$v` functions that take a signal first, with the signal supplied for you: `x.$.rotate(0.1)` reads in signal-flow order and is `$v.warp(x, { rotate: 0.1 })`. `.$m` is the same with a leading `mix` argument that crossfades the signal against the result, 0 for the signal and 1 for the result: `x.$m.kaleid(0.5, 6)`. `.pipe(fn)` calls `fn(signal)`, and `.pipe(fn, array)` calls it once per element and returns the results as an array. `.pipeMix(fn, mix)` crossfades the signal against `fn(signal)`, half way by default. `range(min, max)` is called directly on a signal and maps the 0 to 1 of each channel onto `min` to `max`. `out`, `preview`, `toCV`, `write` and `range` end, tap or rescale a chain and are called directly on the signal. `rotate`, `scale` and `scroll` are shorthand for `warp`.';

export const VIDEO_CHAIN_EXAMPLES: string[] = [
    '$v.osc(10).$.modulate($v.noise(3, $v.time), 0.3).$.kaleid(6).$.hsv().out()',
    '$v.noise(4, $v.time).$.rotate(0.1).$.pixelate(32, 18).$.hsv(0.8).preview().out()',
    '$v.osc(8).$.hsv().$m.kaleid(0.5, 5).out()',
    "$v.ramp().$.osc(8).$.mult($v.ramp('vertical').$.osc(4)).$.hsv().out()",
    '$v.osc(8).$.hsv().pipe((c) => c.$.kaleid(5).$.hueShift(0.3)).out()',
    '$v.osc(8).$.hsv().pipeMix((c) => c.$.invert(), $v.time.$.osc(0.2)).out()',
];

export const VIDEO_CHAIN: VideoChainDoc[] = [
    {
        name: 'add',
        description: 'Sum with `b`, clipped to 0..1.',
        declarations: ['add(b: VideoValue): VideoSignal;'],
    },
    {
        name: 'mult',
        description: 'Product with `b`.',
        declarations: ['mult(b: VideoValue): VideoSignal;'],
    },
    {
        name: 'diff',
        description: 'Absolute difference from `b`.',
        declarations: ['diff(b: VideoValue): VideoSignal;'],
    },
    {
        name: 'max',
        description: 'The larger of this and `b`.',
        declarations: ['max(b: VideoValue): VideoSignal;'],
    },
    {
        name: 'min',
        description: 'The smaller of this and `b`.',
        declarations: ['min(b: VideoValue): VideoSignal;'],
    },
    {
        name: 'mix',
        description: 'Crossfade from this to `b`.',
        declarations: ['mix(b: VideoValue, amount?: VideoValue): VideoSignal;'],
    },
    {
        name: 'invert',
        description: 'The complement, 1 minus this.',
        declarations: ['invert(): VideoSignal;'],
    },
    {
        name: 'range',
        direct: true,
        description:
            "Maps each channel's 0 to 1 onto `min` to `max`: `signal.range(1, 3)` gives values from 1 to 3, for an input in its own units such as a zoom factor or a count.",
        declarations: [
            'range(min?: VideoValue, max?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'posterize',
        description: 'Quantizes to `levels` steps.',
        declarations: ['posterize(levels?: VideoValue): VideoSignal;'],
    },
    {
        name: 'write',
        direct: true,
        description:
            'Stores this in a buffer for the next frame to read, and returns it.',
        declarations: ['write(buffer: VideoBuffer): VideoSignal;'],
    },
    {
        name: 'wrap',
        description: 'Multiplies by `gain`, then keeps the fractional part.',
        declarations: ['wrap(gain?: VideoValue): VideoSignal;'],
    },
    {
        name: 'fold',
        description:
            'Multiplies by `gain`, then reflects what passes 1 back down.',
        declarations: ['fold(gain?: VideoValue): VideoSignal;'],
    },
    {
        name: 'osc',
        description:
            'A gray wave along this signal: `freq` cycles per unit of its brightness, offset by `phase`.',
        declarations: [
            "osc(\n    freq: VideoValue,\n    phase?: VideoValue,\n    config?: { shape?: 'sine' | 'triangle' | 'saw' | 'square' },\n): VideoSignal;",
        ],
    },
    {
        name: 'comparator',
        description: 'Threshold with a soft edge.',
        declarations: [
            'comparator(threshold?: VideoValue, softness?: VideoValue, config?: { invert?: boolean }): VideoSignal;',
        ],
    },
    {
        name: 'hsv',
        description:
            'Uses the brightness of this as the hue of a color, so a ramp paints a rainbow. To color a mask instead, with a fixed hue and this as the brightness, use `tint`.',
        declarations: [
            'hsv(saturation?: VideoValue, value?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'tint',
        description:
            'Colors this: its brightness becomes the value of a color with the given hue and saturation, so a mask turns into a shape of that color.',
        declarations: [
            'tint(hue?: VideoValue, saturation?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'procAmp',
        description: 'Saturation, then gain and bias.',
        declarations: [
            'procAmp(gain?: VideoValue, bias?: VideoValue, saturation?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'hueShift',
        description: 'Turns every hue of this by `amount` of a full circle.',
        declarations: ['hueShift(amount?: VideoValue): VideoSignal;'],
    },
    {
        name: 'contrast',
        description: "Scales this signal's distance from mid-gray by `amount`.",
        declarations: ['contrast(amount?: VideoValue): VideoSignal;'],
    },
    {
        name: 'blur',
        description: 'Averages this over a disk of `radius`.',
        declarations: ['blur(radius?: VideoValue): VideoSignal;'],
    },
    {
        name: 'bloom',
        description:
            'Adds a blurred copy of this back on top of it, so bright areas glow.',
        declarations: [
            'bloom(radius?: VideoValue, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'edges',
        description: 'Brightness of the steepest change around each pixel.',
        declarations: ['edges(amount?: VideoValue): VideoSignal;'],
    },
    {
        name: 'scanlines',
        description: 'Darkens evenly spaced horizontal lines of this.',
        declarations: [
            'scanlines(count?: VideoValue, strength?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'vignette',
        description: 'Darkens this toward the corners of the frame.',
        declarations: [
            'vignette(strength?: VideoValue, radius?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'grain',
        description: 'Adds film grain to this, redrawn every frame.',
        declarations: ['grain(amount?: VideoValue): VideoSignal;'],
    },
    {
        name: 'channel',
        description:
            'One channel of this, or its brightness, as an opaque gray.',
        declarations: [
            "channel(which?: 'r' | 'g' | 'b' | 'a' | 'luma'): VideoSignal;",
        ],
    },
    {
        name: 'swiz',
        description:
            "Reorders the channels of this: each letter of `pattern` names the channel that fills red, green and blue in turn, so `'gbr'` shifts them round and `'aaa'` shows the alpha. The alpha is kept.",
        declarations: ['swiz(pattern: string): VideoSignal;'],
    },
    {
        name: 'frameDelay',
        description:
            'This as it was `frames` frames ago; transparent black until then.',
        declarations: ['frameDelay(frames?: number): VideoSignal;'],
    },
    {
        name: 'key',
        description:
            'Shows this where `mask` is 1 and `background` where it is 0.',
        declarations: [
            'key(background: VideoValue, mask: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'layer',
        description: 'Lays `top` over this by the alpha of `top`.',
        declarations: ['layer(top: VideoValue): VideoSignal;'],
    },
    {
        name: 'mask',
        description:
            'This seen through `mask`: its color and alpha scaled by the brightness of `mask`.',
        declarations: ['mask(mask: VideoValue): VideoSignal;'],
    },
    {
        name: 'luma',
        description:
            'Keys this by its own brightness: opaque from `threshold` up, transparent and black below `threshold - tolerance`; `config.invert` keeps the dark side.',
        declarations: [
            'luma(threshold?: VideoValue, tolerance?: VideoValue, config?: { invert?: boolean }): VideoSignal;',
        ],
    },
    {
        name: 'warp',
        description: 'Zooms, turns and shifts everything this draws.',
        declarations: [
            'warp(config?: { zoom?: VideoValue; rotate?: VideoValue; shiftX?: VideoValue; shiftY?: VideoValue }): VideoSignal;',
        ],
    },
    {
        name: 'rotate',
        description:
            'Turns everything this draws by `turns`; positive is clockwise.',
        declarations: ['rotate(turns: VideoValue): VideoSignal;'],
    },
    {
        name: 'scale',
        description: 'Zooms everything this draws by `zoom` about the center.',
        declarations: ['scale(zoom: VideoValue): VideoSignal;'],
    },
    {
        name: 'scroll',
        description:
            'Shifts everything this draws right by `x` and up by `y`, as fractions of the frame.',
        declarations: ['scroll(x?: VideoValue, y?: VideoValue): VideoSignal;'],
    },
    {
        name: 'displace',
        description: 'Reads this at positions pushed by `dx` and `dy`.',
        declarations: [
            'displace(dx: VideoValue, dy?: VideoValue, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulate',
        description: 'Pushes this around by another signal.',
        declarations: [
            'modulate(modulator: VideoSignal, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateScale',
        description:
            'Scales this about the center by `offset + multiple * channel` of a modulator.',
        declarations: [
            'modulateScale(modulator: VideoSignal, multiple?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateRotate',
        description:
            'Turns this about the center by `offset + multiple * red` of a modulator.',
        declarations: [
            'modulateRotate(modulator: VideoSignal, multiple?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulatePixelate',
        description:
            'Holds this constant over a grid of `offset + multiple * channel` cells.',
        declarations: [
            'modulatePixelate(modulator: VideoSignal, multiple?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateKaleid',
        description:
            'Mirrors this into `sides` wedges, pushed outward by a modulator.',
        declarations: [
            'modulateKaleid(modulator: VideoSignal, sides?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateHue',
        description:
            "Pushes this by the differences between a modulator's color channels.",
        declarations: [
            'modulateHue(modulator: VideoSignal, amount?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateRepeat',
        description:
            'Tiles this with alternate rows and columns offset by a modulator.',
        declarations: [
            'modulateRepeat(modulator: VideoSignal, repeatX?: VideoValue, repeatY?: VideoValue, offsetX?: VideoValue, offsetY?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateRepeatX',
        description:
            'Tiles this across, shifting alternate columns by a modulator.',
        declarations: [
            'modulateRepeatX(modulator: VideoSignal, reps?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateRepeatY',
        description: 'Tiles this up, shifting alternate rows by a modulator.',
        declarations: [
            'modulateRepeatY(modulator: VideoSignal, reps?: VideoValue, offset?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateScrollX',
        description: 'Scrolls this across by a modulator, wrapping.',
        declarations: [
            'modulateScrollX(modulator: VideoSignal, scroll?: VideoValue, speed?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'modulateScrollY',
        description: 'Scrolls this up by a modulator, wrapping.',
        declarations: [
            'modulateScrollY(modulator: VideoSignal, scroll?: VideoValue, speed?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'kaleid',
        description: 'Mirrors this around the center into `sides` wedges.',
        declarations: ['kaleid(sides?: VideoValue): VideoSignal;'],
    },
    {
        name: 'pixelate',
        description: 'Holds this constant across a grid of `x` by `y` cells.',
        declarations: [
            'pixelate(x?: VideoValue, y?: VideoValue): VideoSignal;',
        ],
    },
    {
        name: 'repeat',
        description: 'Tiles this `x` by `y` times.',
        declarations: ['repeat(x?: VideoValue, y?: VideoValue): VideoSignal;'],
    },
    {
        name: 'preview',
        direct: true,
        description: 'Shows this in the editor and returns it.',
        declarations: [
            "preview(config?: { view?: 'image' | 'waveform' | 'vectorscope' }): this;",
        ],
    },
    {
        name: 'toCV',
        direct: true,
        description: 'Averages a region of this into an audio control signal.',
        declarations: [
            'toCV(config?: { x?: number; y?: number; size?: number }): CollectionWithRange;',
        ],
    },
    {
        name: 'out',
        direct: true,
        description:
            'Shows this behind the code, and in the performance window when it is open, laid over black by its alpha. Returns this unchanged, so the chain can go on.',
        declarations: ['out(): this;'],
    },
];
