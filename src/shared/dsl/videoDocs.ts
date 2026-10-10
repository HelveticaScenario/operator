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
        'Video synthesis. Patches build a graph of fields that is drawn per pixel behind the code in the editor when a patch calls `$v.out`. View, Toggle Performance Window (Ctrl+Shift+V) opens a second window that shows the same picture for an audience; while it is open, the picture behind the code takes the shape and resolution of that window. Video signals cannot be connected to audio inputs.\n\nFields are in volts, as audio signals are: a field runs from 0 to 5, and 5 is full. A ramp runs from 0 to 5 across the frame, a color channel at 5 is fully on, and a strength or an amount of 5 is all of it. Angles and phases are fractions of 5 too: `rotate: 5` is a full turn and `phase: 2.5` half a cycle. Counts and factors keep their own units: `freq: 10` is ten cycles, `zoom: 2` doubles the size, `kaleid(6)` is six wedges.\n\nSliders, buttons and audio signals can be passed anywhere a field is accepted, and the picture follows them live. So can patterns from `$p(...)`, `$p.s(...)` and `$p.arrange(...)`, which play through their own `$cycle` as they do for any audio parameter; their values are volts, so `$p("0 1.25 2.5 3.75")` steps through four hues a cycle. Audio signals are sampled about 60 times a second and used in volts as the audio graph produces them, so a 0 to 5 volt signal fills a full-scale input as it is; `.range(min, max)`, on an audio signal or on a field, sets any other range, such as `.range(1, 3)` for a zoom of one to three times. Pass one channel; a polyphonic signal is rejected.',
    examples: [
        '$v.osc($v.ramp(), 10).$.kaleid(6).$.hsv().out()',
        "$v.out($v.hsv($slider('Hue', 1.5, 0, 5), 5, $slider('Level', 5, 0, 5)))",
        "$v.out($v.hsv($p('0 1.25 2.5 3.75'), 5, $p.arrange([2, $p('5 2.5')], [1, $p('5')])))",
        "$v.out($v.hsv($sine('0.2hz').range(0, 5), 5, $v.shape($v.ramp(), $v.ramp('v'), $sine('1hz').range(0.5, 2))))",
    ],
};

export const VIDEO_DOCS: VideoDoc[] = [
    {
        name: 'time',
        group: 'Generators',
        description:
            'Seconds since the patch started, as volts: it reaches 5 after five seconds. It stands still while the patch is stopped and starts again from 0 when the patch is restarted. Use it as a phase to animate an oscillator; an oscillator with a `freq` of 5 fed `$v.time` completes a cycle every second.',
        params: [],
        examples: ['$v.out($v.colorize($v.osc($v.ramp(), 4, $v.time), 0, 0))'],
        declarations: ['readonly time: VideoField;'],
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
                    "`'h'` horizontal, 0 to 5 across the frame (default), `'v'` vertical, 0 to 5 up the frame, `'d'` diagonal, `'r'` distance from the center (2.5 at the top and bottom edges), `'a'` angle around the center, 0 to 5 once around",
            },
            {
                name: 'config.zoom',
                description:
                    'Magnification about the center; above 1 enlarges the pattern (default 1)',
            },
            {
                name: 'config.rotate',
                description:
                    'Rotation, with 5 a full turn; positive turns the pattern clockwise (default 0)',
            },
            {
                name: 'config.shiftX',
                description:
                    'Horizontal move, with 5 the whole width; positive moves right (default 0)',
            },
            {
                name: 'config.shiftY',
                description:
                    'Vertical move, with 5 the whole height; positive moves up (default 0)',
            },
        ],
        examples: [
            "$v.out($v.colorize($v.ramp(), $v.ramp('v'), 2.5))",
            "$v.out($v.hsv($v.osc($v.ramp('r'), 6, $v.time)))",
            "$v.out($v.hsv($v.ramp('a', { rotate: $v.osc($v.time, 0.5) })))",
            "$v.out($v.colorize($v.osc($v.ramp('h', { rotate: 0.625, zoom: 2 }), 8), 1, 2.5))",
        ],
        declarations: [
            "ramp(\n    axis?: 'h' | 'v' | 'd' | 'r' | 'a',\n    config?: {\n        zoom?: VideoValue;\n        rotate?: VideoValue;\n        shiftX?: VideoValue;\n        shiftY?: VideoValue;\n    },\n): VideoField;",
        ],
    },
    {
        name: 'osc',
        group: 'Generators',
        description:
            'Periodic shaper from 0 to 5: `freq` cycles across the full 5 volts of `input`, offset by `phase`, with 5 a full cycle. A ramp spans 5 volts, so a `freq` of 8 on a ramp draws eight stripes.',
        params: [
            { name: 'input', description: 'Field to shape, usually a ramp' },
            {
                name: 'freq',
                description:
                    'Cycles across 5 volts of `input`; fed `$v.time`, which counts seconds, 5 is one cycle a second',
            },
            {
                name: 'phase',
                description: 'Offset, with 5 a full cycle (default 0)',
            },
            {
                name: 'config.shape',
                description:
                    "`'sine'` (default), `'triangle'`, `'saw'` or `'square'`",
            },
        ],
        examples: [
            '$v.out($v.colorize($v.osc($v.ramp(), 8), 0, 0))',
            "$v.out($v.colorize($v.osc($v.ramp(), 3, $v.time, { shape: 'saw' }), $v.osc($v.ramp('v'), 5), 1.5))",
        ],
        declarations: [
            "osc(\n    input: VideoValue,\n    freq: VideoValue,\n    phase?: VideoValue,\n    config?: { shape?: 'sine' | 'triangle' | 'saw' | 'square' },\n): VideoField;",
        ],
    },
    {
        name: 'fromAudio',
        group: 'Generators',
        description:
            'The recent audio-rate samples of an audio signal, laid along `position`: 0 is the oldest sample in the window and 1 the newest. Values are the volts the audio graph produces, interpolated between samples. With the default horizontal ramp for `position` this is a scope: compare it with the vertical ramp to draw the wave. Unlike an audio signal used directly as an input, which is read once per frame, this shows every sample.',
        params: [
            {
                name: 'signal',
                description:
                    'A single-channel audio signal, such as an oscillator or an envelope, or a pattern from `$p(...)`',
            },
            {
                name: 'position',
                description:
                    'Where along the window to read, 0 to 5 (default the horizontal ramp)',
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
            "$v.out($v.hsv(1.75, 5, $v.invert($v.comparator($v.diff($v.ramp('v'), $v.add(2.5, $v.mult($v.fromAudio($sine('110hz')), 2))), 0.05, 0.05))))",
            "$v.out($v.hsv($v.fromAudio($saw('55hz'), $v.ramp('r'), { samples: 1024 }), 5, 5))",
        ],
        declarations: [
            'fromAudio(signal: ModuleOutput | Collection | CollectionWithRange | PatternSource, position?: VideoValue, config?: { samples?: number; trigger?: boolean }): VideoField;',
        ],
    },
    {
        name: 'noise',
        group: 'Generators',
        description:
            'Smooth value noise from 0 to 5. One volt of any coordinate is one cell of the noise, so a ramp, which spans 5 volts, crosses five cells; scaling the coordinates sets the grain, and `z` moves through the noise, which animates it when fed `$v.time`.',
        params: [
            { name: 'x', description: 'Horizontal coordinate' },
            { name: 'y', description: 'Vertical coordinate' },
            {
                name: 'z',
                description:
                    'Position through the noise, such as time (default 0)',
            },
        ],
        examples: [
            "$v.out($v.hsv($v.noise($v.mult($v.ramp(), 6), $v.mult($v.ramp('v'), 6), $v.mult($v.time, 2.5))))",
            "$v.out($v.hsv(3, 5, $v.comparator($v.noise($v.mult($v.ramp('r'), 10), $v.mult($v.ramp('a'), 6), $v.time), 2.5, 0.25)))",
        ],
        declarations: [
            'noise(x: VideoValue, y: VideoValue, z?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'voronoi',
        group: 'Generators',
        description:
            'Cellular noise: the distance to the nearest of a scatter of points, one per unit cell of the coordinates, between 0 (on a point) and about 5. One volt of any coordinate is one cell, so scale the coordinates to set the cell size; `z` moves the points, which animates the cells when fed `$v.time`.',
        params: [
            { name: 'x', description: 'Horizontal coordinate' },
            { name: 'y', description: 'Vertical coordinate' },
            {
                name: 'z',
                description: 'Moves the points, such as time (default 0)',
            },
        ],
        examples: [
            "$v.out($v.hsv(2.75, 4, $v.voronoi($v.mult($v.ramp(), 8), $v.mult($v.ramp('v'), 5), $v.mult($v.time, 10))))",
            "$v.voronoi($v.mult($v.ramp(), 6), $v.mult($v.ramp('v'), 6), $v.time).$.invert().$.tint(0.5).out()",
        ],
        declarations: [
            'voronoi(x: VideoValue, y: VideoValue, z?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'polygon',
        group: 'Generators',
        description:
            '5 inside a regular polygon centered on (x, y) with one point up, 0 outside, with a soft edge. Pass ramps for x and y to center it on the frame.',
        params: [
            {
                name: 'x',
                description: 'Horizontal position field; 2.5 is the center',
            },
            {
                name: 'y',
                description: 'Vertical position field; 2.5 is the center',
            },
            {
                name: 'sides',
                description: 'Number of sides, at least 3 (default 3)',
            },
            {
                name: 'size',
                description:
                    'Distance from the center to the middle of a side, with 5 the whole frame height (default 1.25)',
            },
            {
                name: 'softness',
                description:
                    'Width of the edge in the same units (default 0.05)',
            },
        ],
        examples: [
            "$v.out($v.hsv(0.6, 5, $v.polygon($v.ramp(), $v.ramp('v'), 5, 1.5, 0.1)))",
            "$v.polygon($v.ramp(), $v.ramp('v'), 6, 1).$.rotate($v.mult($v.time, 2.5)).$.tint(3).out()",
        ],
        declarations: [
            'polygon(x: VideoValue, y: VideoValue, sides?: VideoValue, size?: VideoValue, softness?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'blur',
        group: 'Filters',
        description:
            "Averages `input` over a disk of `radius`, softening it. The input's whole sub-patch is read at sixteen nearby coordinates, so its cost is paid sixteen times: blur cheap things, or blur late. A field or color in gives the same type out. Adding a blurred copy back on top of the original is a glow.",
        params: [
            {
                name: 'radius',
                description:
                    'Radius of the disk, with 5 the whole frame height (default 0.05)',
            },
        ],
        examples: [
            "$v.out($v.hsv($v.blur($v.polygon($v.ramp(), $v.ramp('v'), 5, 1, 0.005), 0.2)))",
            "$v.polygon($v.ramp(), $v.ramp('v'), 6, 0.9, 0.01).$.tint(0.6).pipe((c) => c.$.add(c.$.blur(0.2).$.mult(10))).out()",
        ],
        declarations: [
            'blur(input: VideoField, radius?: VideoValue): VideoField;',
            'blur(input: VideoColor, radius?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'bloom',
        group: 'Filters',
        description:
            'Adds a blurred copy of `input` back on top of it, so bright areas glow. It costs one blur. A field or color in gives the same type out.',
        params: [
            {
                name: 'radius',
                description:
                    'Radius of the blur, with 5 the whole frame height (default 0.2)',
            },
            {
                name: 'amount',
                description:
                    'How strongly the blurred copy is added back; 5 adds all of it (default 5)',
            },
        ],
        examples: [
            "$v.out($v.bloom($v.hsv(0.5, 5, $v.polygon($v.ramp(), $v.ramp('v'), 5, 0.75, 0.01)), 0.25, 10))",
            "$v.polygon($v.ramp(), $v.ramp('v'), 6, 0.75, 0.01).$.tint(2.75).$.bloom(0.3, 12.5).out()",
        ],
        declarations: [
            'bloom(input: VideoField, radius?: VideoValue, amount?: VideoValue): VideoField;',
            'bloom(input: VideoColor, radius?: VideoValue, amount?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'edges',
        group: 'Filters',
        description:
            'Brightness of the steepest change around each pixel (a Sobel filter): 0 on flat areas, rising along edges. It reads `input` at the eight neighbors one pixel away, so it sees the same pixel grid at any window size. A color contributes its brightness.',
        params: [
            {
                name: 'amount',
                description:
                    'Gain on the result, which is clipped to 0 to 5; 5 leaves it as it is (default 5)',
            },
        ],
        examples: [
            "$v.out($v.hsv(2, 5, $v.edges($v.noise($v.mult($v.ramp(), 6), $v.mult($v.ramp('v'), 6), $v.time), 20)))",
        ],
        declarations: [
            'edges(input: VideoField | VideoColor, amount?: VideoValue): VideoField;',
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
            "$v.image('pictures/photo.png', { fit: 'contain' }).$.kaleid(6).$.hueShift($v.osc($v.time, 0.25)).out()",
        ],
        declarations: [
            "image(path: string, config?: { fit?: 'cover' | 'contain' | 'stretch' }): VideoColor;",
        ],
    },
    {
        name: 'video',
        group: 'Generators',
        description:
            "A recording from the workspace folder (`.mp4`, `.webm`, `.mov`, `.m4v` or `.ogv`), or a video or stream at an `http://` or `https://` URL, as a color, played in a loop with its sound off. A URL can be a video file, or an HLS stream when it ends in `.m3u8`; RTSP, RTMP and other protocols browsers cannot play are refused. The app fetches the URL itself, so the server needs no special headers. Like `$v.image`, it is read at the coordinate being drawn. It plays while the patch plays, holds its frame when the patch is stopped, and starts over from its loop start when the patch starts. Changing `speed` or `loop` and running the patch again keeps the video playing from where it is. Which formats play depends on the codecs the app's browser engine includes (H.264, HEVC, VP8/VP9 and AV1); `speed` and `loop` are fixed numbers, and video cannot play backwards.",
        params: [
            {
                name: 'path',
                description:
                    'File to play, relative to the workspace folder, or an http(s) URL of a video or an .m3u8 HLS stream',
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
            "$v.video('clips/loop.mp4').$.mult($v.hsv($v.ramp(), 2.5, 5)).$.warp({ rotate: 0.1 }).out()",
            "$v.video('clips/loop.mp4', { speed: 0.5, loop: [1, 2.5] }).out()",
            "$v.video('https://example.com/live/stream.m3u8', { fit: 'contain' }).$.hueShift($v.osc($v.time, 0.25)).out()",
        ],
        declarations: [
            "video(\n    path: string,\n    config?: {\n        fit?: 'cover' | 'contain' | 'stretch';\n        speed?: number;\n        loop?: [start: number, end?: number];\n    },\n): VideoColor;",
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
            "$v.camera({ fit: 'contain' }).$.kaleid(6).$.hueShift($v.osc($v.time, 0.25)).out()",
        ],
        declarations: [
            "camera(config?: { device?: string; fit?: 'cover' | 'contain' | 'stretch' }): VideoColor;",
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
            "$v.screen({ display: 2, fit: 'contain' }).$.hueShift($v.osc($v.time, 0.25)).out()",
        ],
        declarations: [
            "screen(config?: { display?: number; fit?: 'cover' | 'contain' | 'stretch' }): VideoColor;",
        ],
    },
    {
        name: 'shape',
        group: 'Generators',
        description:
            '5 inside a shape and 0 outside, with a soft edge. Pass ramps for x and y to center it on the frame; offset them to move it. Circles stay round at any window aspect.',
        params: [
            {
                name: 'x',
                description: 'Horizontal position field; 2.5 is the center',
            },
            {
                name: 'y',
                description: 'Vertical position field; 2.5 is the center',
            },
            {
                name: 'size',
                description:
                    'Half-extent, with 5 the whole frame height (default 1.25)',
            },
            {
                name: 'softness',
                description:
                    'Width of the edge in the same units (default 0.05)',
            },
            {
                name: 'config.shape',
                description: "`'circle'` (default), `'box'` or `'diamond'`",
            },
        ],
        examples: [
            "$v.out($v.hsv(3, 5, $v.shape($v.ramp(), $v.ramp('v'), 1.5, 0.25)))",
            "$v.out($v.hsv($v.time, 5, $v.shape($v.ramp(), $v.ramp('v'), 1, 0, { shape: 'diamond' })))",
        ],
        declarations: [
            "shape(\n    x: VideoValue,\n    y: VideoValue,\n    size?: VideoValue,\n    softness?: VideoValue,\n    config?: { shape?: 'circle' | 'box' | 'diamond' },\n): VideoField;",
        ],
    },
    {
        name: 'colorize',
        group: 'Color',
        description:
            'Combines three fields into a color. Each channel runs from 0 (off) to 5 (full) and is clipped to that.',
        params: [],
        examples: ["$v.out($v.colorize($v.ramp(), $v.ramp('v'), 5))"],
        declarations: [
            'colorize(r: VideoValue, g: VideoValue, b: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'hsv',
        group: 'Color',
        description:
            'Color from hue, saturation and value. Hue wraps every 5, so a ramp across the frame is one trip round the rainbow; saturation and value run from 0 to 5 and are clipped to that.',
        params: [],
        examples: [
            '$v.out($v.hsv($v.ramp(), 5, 5))',
            "$v.out($v.hsv($v.osc($v.ramp(), 2, $v.time), 4, $v.ramp('v')))",
        ],
        declarations: [
            'hsv(h: VideoValue, s?: VideoValue, v?: VideoValue): VideoColor;',
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
                description: 'Color (a field is treated as gray)',
            },
            {
                name: 'gain',
                description: 'Multiplier; 5 leaves it as it is (default 5)',
            },
            {
                name: 'bias',
                description:
                    'Offset added after gain, with 5 the whole range (default 0)',
            },
            {
                name: 'saturation',
                description: '0 is gray, 5 is unchanged (default 5)',
            },
        ],
        examples: ['$v.out($v.procAmp($v.hsv($v.ramp()), 7.5, -1, 2.5))'],
        declarations: [
            'procAmp(\n    input: VideoSignal,\n    gain?: VideoValue,\n    bias?: VideoValue,\n    saturation?: VideoValue,\n): VideoColor;',
        ],
    },
    {
        name: 'add',
        group: 'Math',
        description: 'Sum, clipped to 0 to 5. Colors add per channel.',
        params: [],
        examples: [
            "$v.out($v.hsv($v.add($v.ramp(), $v.osc($v.ramp('v'), 3))))",
        ],
        declarations: [
            'add(a: VideoValue, b: VideoValue): VideoField;',
            'add(a: VideoSignal, b: VideoSignal): VideoColor;',
        ],
    },
    {
        name: 'mult',
        group: 'Math',
        description:
            'Product, as a fraction of 5: multiplying by 5 changes nothing and by 2.5 halves. Colors multiply per channel.',
        params: [],
        examples: ["$v.out($v.mult($v.hsv($v.ramp()), $v.ramp('v')))"],
        declarations: [
            'mult(a: VideoValue, b: VideoValue): VideoField;',
            'mult(a: VideoSignal, b: VideoSignal): VideoColor;',
        ],
    },
    {
        name: 'diff',
        group: 'Math',
        description: 'Absolute difference. Colors differ per channel.',
        params: [],
        examples: ["$v.out($v.hsv($v.diff($v.ramp(), $v.ramp('v'))))"],
        declarations: [
            'diff(a: VideoValue, b: VideoValue): VideoField;',
            'diff(a: VideoSignal, b: VideoSignal): VideoColor;',
        ],
    },
    {
        name: 'max',
        group: 'Math',
        description:
            'The larger of two values. With shapes this is their union; colors take the larger channel.',
        params: [],
        examples: [
            "$v.out($v.hsv(0.5, 5, $v.max($v.shape($v.ramp(), $v.ramp('v'), 1), $v.shape($v.add($v.ramp(), 1), $v.ramp('v'), 1))))",
        ],
        declarations: [
            'max(a: VideoValue, b: VideoValue): VideoField;',
            'max(a: VideoSignal, b: VideoSignal): VideoColor;',
        ],
    },
    {
        name: 'min',
        group: 'Math',
        description:
            'The smaller of two values. With shapes this is their intersection; colors take the smaller channel.',
        params: [],
        examples: [
            "$v.out($v.hsv(2.5, 5, $v.min($v.shape($v.ramp(), $v.ramp('v'), 1.5), $v.shape($v.add($v.ramp(), 1), $v.ramp('v'), 1.5))))",
        ],
        declarations: [
            'min(a: VideoValue, b: VideoValue): VideoField;',
            'min(a: VideoSignal, b: VideoSignal): VideoColor;',
        ],
    },
    {
        name: 'invert',
        group: 'Math',
        description: 'Complement, 5 minus the input.',
        params: [],
        examples: ['$v.out($v.invert($v.hsv($v.ramp())))'],
        declarations: [
            'invert(input: VideoValue): VideoField;',
            'invert(input: VideoSignal): VideoColor;',
        ],
    },
    {
        name: 'mix',
        group: 'Math',
        description: 'Crossfade from `a` (amount 0) to `b` (amount 5).',
        params: [
            {
                name: 'amount',
                description: 'Mix position, clipped to 0 to 5 (default 2.5)',
            },
        ],
        examples: [
            "$v.out($v.mix($v.hsv($v.ramp()), $v.hsv($v.ramp('v')), $v.osc($v.time, 1.25)))",
        ],
        declarations: [
            'mix(a: VideoValue, b: VideoValue, amount?: VideoValue): VideoField;',
            'mix(a: VideoSignal, b: VideoSignal, amount?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'range',
        group: 'Shaping',
        description:
            "Maps a field's 0 to 5 volts onto `min` to `max` volts, as `.range` does for an audio signal. Use it to give an input that has its own units, such as a zoom factor, a frequency or a count, the span you want. It is also called on the field itself, as `field.range(min, max)`.",
        params: [
            { name: 'min', description: 'The value 0 becomes (default 0)' },
            { name: 'max', description: 'The value 5 becomes (default 5)' },
        ],
        examples: [
            "$v.out($v.hsv($v.ramp('a', { zoom: $v.range($v.osc($v.time, 0.5), 1, 3) })))",
            '$v.out($v.hsv($v.ramp().range(0, 2.5)))',
        ],
        declarations: [
            'range(input: VideoField, min?: VideoValue, max?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'wrap',
        group: 'Shaping',
        description:
            'Multiplies by `gain`, then keeps the fractional part: a sawtooth of the input that repeats `gain` times as the input goes from 0 to 5.',
        params: [],
        examples: ['$v.out($v.hsv($v.wrap($v.ramp(), 4)))'],
        declarations: [
            'wrap(input: VideoValue, gain?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'fold',
        group: 'Shaping',
        description:
            'Multiplies by `gain`, then reflects whatever passes 5 back down: a triangle of the input that matches it where it stays under 5 / gain.',
        params: [],
        examples: ['$v.out($v.hsv($v.fold($v.ramp(), 3)))'],
        declarations: [
            'fold(input: VideoValue, gain?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'comparator',
        group: 'Shaping',
        description:
            'Threshold: 0 below `threshold`, 5 above, with a linear ramp `softness` wide centered on it (a hard edge when 0). Both are in volts; the threshold defaults to 2.5.',
        params: [],
        examples: [
            '$v.out($v.hsv(0.5, 5, $v.comparator($v.osc($v.ramp(), 4), 2.5, 0.5)))',
        ],
        declarations: [
            'comparator(\n    input: VideoValue,\n    threshold?: VideoValue,\n    softness?: VideoValue,\n): VideoField;',
        ],
    },
    {
        name: 'posterize',
        group: 'Shaping',
        description:
            'Quantizes to `levels` evenly spaced values between 0 and 5.',
        params: [
            {
                name: 'levels',
                description: 'Number of steps, at least 2 (default 4)',
            },
        ],
        examples: ['$v.out($v.hsv($v.posterize($v.ramp(), 5)))'],
        declarations: [
            'posterize(input: VideoValue, levels?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'key',
        group: 'Compositing',
        description: 'Shows `fg` where `mask` is 5 and `bg` where it is 0.',
        params: [],
        examples: [
            "$v.out($v.key($v.hsv(4.5), $v.hsv($v.ramp(), 5, 2.5), $v.shape($v.ramp(), $v.ramp('v'), 1.5)))",
        ],
        declarations: [
            'key(fg: VideoSignal, bg: VideoSignal, mask: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'feedback',
        group: 'Memory',
        description:
            "Feeds a frame back into itself. `update` receives the previous frame's result as `prev`, resampled through the transform in `config`, and returns this frame's color; `feedback` returns that color. The first frame's `prev` is black. Re-running a patch keeps the loops' pictures, so edits take effect without wiping the trails.",
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
                    'Rotation per frame, with 5 a full turn; positive turns the picture clockwise (default 0)',
            },
            {
                name: 'config.shiftX',
                description:
                    'Horizontal move per frame, with 5 the whole width; positive moves right (default 0)',
            },
            {
                name: 'config.shiftY',
                description:
                    'Vertical move per frame, with 5 the whole height; positive moves up (default 0)',
            },
            {
                name: 'config.edge',
                description:
                    "What lies beyond the frame border: `'clamp'` (default), `'repeat'` or `'mirror'`",
            },
        ],
        examples: [
            "$v.out($v.feedback((prev) => $v.mix($v.hsv($v.time, 5, $v.shape($v.ramp(), $v.ramp('v'), 0.5)), prev, 4.5), { zoom: 1.02, rotate: 0.02 }))",
            "$v.out($v.feedback((prev) => $v.add($v.hsv($v.osc($v.time, 1), 5, $v.shape($v.add($v.ramp(), -1), $v.ramp('v'), 0.25)), $v.mult(prev, 4.8)), { rotate: 0.05 }))",
        ],
        declarations: [
            "feedback(\n    update: (prev: VideoColor) => VideoSignal,\n    config?: {\n        zoom?: VideoValue;\n        rotate?: VideoValue;\n        shiftX?: VideoValue;\n        shiftY?: VideoValue;\n        edge?: 'clamp' | 'repeat' | 'mirror';\n    },\n): VideoColor;",
        ],
    },
    {
        name: 'frameDelay',
        group: 'Memory',
        description:
            '`input` as it was `frames` frames ago, always as a color (a field comes out gray); until that many frames have passed it is black. Add it back to the live picture for echoes and trails. Each frame held back takes one of the seven frame buffers that feedback loops and `$v.buffer` also use, so a long delay leaves fewer for them.',
        params: [
            {
                name: 'frames',
                description:
                    'Whole number of frames to hold back, 1 to 7 (default 1)',
            },
        ],
        examples: [
            "const dot = $v.hsv(1.5, 5, $v.shape($v.add($v.ramp(), $v.osc($v.time, 2.5)), $v.ramp('v'), 0.6)); $v.out($v.add(dot, $v.frameDelay(dot, 6).$.mult(2.5)))",
        ],
        declarations: [
            'frameDelay(input: VideoSignal, frames?: number): VideoColor;',
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
                    "How the panel draws the signal: `'image'` (default) shows it as a picture, `'waveform'` plots brightness against horizontal position like a waveform monitor, `'vectorscope'` plots every pixel's color by hue and saturation",
            },
        ],
        examples: [
            '$v.out($v.hsv($v.preview($v.osc($v.ramp(), 4))))',
            "$v.out($v.preview($v.hsv($v.ramp('r')), { view: 'waveform' }))",
            "$v.out($v.preview($v.hsv($v.ramp('a')), { view: 'vectorscope' }))",
        ],
        declarations: [
            "preview(signal: VideoField, config?: { view?: 'image' | 'waveform' | 'vectorscope' }): VideoField;",
            "preview(signal: VideoColor, config?: { view?: 'image' | 'waveform' | 'vectorscope' }): VideoColor;",
        ],
    },
    {
        name: 'toCV',
        group: 'Output',
        description:
            'Averages a region of a signal each frame into an audio control signal from 0 to 5 volts, so the picture can modulate the sound: black is 0 and white is 5. A color contributes its brightness. The value is read from the picture about 30 times a second. Scale the result with `.range(min, max)` like any ranged signal.',
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
            "$sine($v.toCV($v.shape($v.ramp(), $v.ramp('v'), 1.5), { size: 0.1 }).range(110, 440)).out()",
        ],
        declarations: [
            'toCV(signal: VideoField | VideoColor, config?: { x?: number; y?: number; size?: number }): CollectionWithRange;',
        ],
    },
    {
        name: 'warp',
        group: 'Warping',
        description:
            'Zooms about the center, turns and shifts everything `input` draws: not just its output but the whole sub-patch behind it, which is evaluated again at the moved coordinates. A field or color in gives the same type out.',
        params: [
            {
                name: 'config.zoom',
                description:
                    'Magnification about the center; above 1 enlarges (default 1)',
            },
            {
                name: 'config.rotate',
                description:
                    'Rotation, with 5 a full turn; positive turns clockwise (default 0)',
            },
            {
                name: 'config.shiftX',
                description:
                    'Horizontal move, with 5 the whole width; positive moves right (default 0)',
            },
            {
                name: 'config.shiftY',
                description:
                    'Vertical move, with 5 the whole height; positive moves up (default 0)',
            },
        ],
        examples: [
            "$v.out($v.hsv($v.warp($v.noise($v.mult($v.ramp(), 4), $v.mult($v.ramp('v'), 4)), { rotate: $v.mult($v.time, 1.25), zoom: 2 })))",
            "$v.out($v.warp($v.hsv($v.shape($v.ramp(), $v.ramp('v'), 0.75)), { shiftX: $v.osc($v.time, 1.25), rotate: 0.5 }))",
        ],
        declarations: [
            'warp(input: VideoField, config?: { zoom?: VideoValue; rotate?: VideoValue; shiftX?: VideoValue; shiftY?: VideoValue }): VideoField;',
            'warp(input: VideoColor, config?: { zoom?: VideoValue; rotate?: VideoValue; shiftX?: VideoValue; shiftY?: VideoValue }): VideoColor;',
        ],
    },
    {
        name: 'displace',
        group: 'Warping',
        description:
            'Reads `input` at positions pushed by `dx` and `dy`, so a moving or noisy push wobbles whatever `input` draws. 2.5 means no push in that direction.',
        params: [
            {
                name: 'dx',
                description:
                    'Horizontal push; below 2.5 pulls left, above pushes right',
            },
            {
                name: 'dy',
                description: 'Vertical push, the same way (default 2.5)',
            },
            {
                name: 'amount',
                description:
                    'Largest push, with 5 the whole frame (default 0.5)',
            },
        ],
        examples: [
            "$v.out($v.hsv($v.displace($v.osc($v.ramp(), 8), $v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time), 2.5, 1)))",
        ],
        declarations: [
            'displace(input: VideoField, dx: VideoValue, dy?: VideoValue, amount?: VideoValue): VideoField;',
            'displace(input: VideoColor, dx: VideoValue, dy?: VideoValue, amount?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulate',
        group: 'Warping',
        description:
            "Pushes `input` around by another signal, as Hydra's `modulate` does. A color moves it by its red and green channels, a field by its value in both directions. It is `displace` with the push taken from a signal.",
        params: [
            {
                name: 'modulator',
                description: 'The signal that pushes `input`',
            },
            {
                name: 'amount',
                description:
                    'Largest push, with 5 the whole frame (default 0.5)',
            },
        ],
        examples: [
            "$v.out($v.hsv($v.modulate($v.osc($v.ramp(), 10), $v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time), 1.5)))",
            "$v.out($v.modulate($v.hsv($v.ramp('r')), $v.hsv($v.noise($v.mult($v.ramp(), 4), $v.mult($v.ramp('v'), 4), 0)), 1))",
        ],
        declarations: [
            'modulate(input: VideoField, modulator: VideoField | VideoColor, amount?: VideoValue): VideoField;',
            'modulate(input: VideoColor, modulator: VideoField | VideoColor, amount?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulateScale',
        group: 'Warping',
        description:
            "As Hydra's `modulateScale`: scales `input` about the center by `offset + multiple * channel`, the modulator's red channel setting the horizontal factor and its green the vertical (a field sets both). Where the modulator is 0 the factor is `offset`, and where it is full it is `offset + multiple`.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
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
            "$v.out($v.modulateScale($v.hsv($v.osc($v.ramp('r'), 10)), $v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time), 1.5, 1))",
        ],
        declarations: [
            'modulateScale(input: VideoField, modulator: VideoField | VideoColor, multiple?: VideoValue, offset?: VideoValue): VideoField;',
            'modulateScale(input: VideoColor, modulator: VideoField | VideoColor, multiple?: VideoValue, offset?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulateRotate',
        group: 'Warping',
        description:
            "As Hydra's `modulateRotate`: turns `input` about the center by `offset + multiple * red`, so the brighter parts of the modulator turn the picture further. Angles are fractions of 5: 5 is a full turn.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
            },
            {
                name: 'multiple',
                description:
                    'Turn at a full modulator, with 5 a full turn (default 5)',
            },
            {
                name: 'offset',
                description: 'Turn where the modulator is 0 (default 0)',
            },
        ],
        examples: [
            "$v.out($v.modulateRotate($v.hsv($v.osc($v.ramp(), 8)), $v.osc($v.ramp('r'), 3, $v.time), 2.5))",
        ],
        declarations: [
            'modulateRotate(input: VideoField, modulator: VideoField | VideoColor, multiple?: VideoValue, offset?: VideoValue): VideoField;',
            'modulateRotate(input: VideoColor, modulator: VideoField | VideoColor, multiple?: VideoValue, offset?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulatePixelate',
        group: 'Warping',
        description:
            "As Hydra's `modulatePixelate`: holds `input` constant over a grid of `offset + multiple * channel` cells, red setting the cells across and green the cells up, so the pixels grow large where the modulator is dark.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
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
            "$v.out($v.modulatePixelate($v.hsv($v.noise($v.mult($v.ramp(), 4), $v.mult($v.ramp('v'), 4), $v.time)), $v.ramp(), 30, 4))",
        ],
        declarations: [
            'modulatePixelate(input: VideoField, modulator: VideoField | VideoColor, multiple?: VideoValue, offset?: VideoValue): VideoField;',
            'modulatePixelate(input: VideoColor, modulator: VideoField | VideoColor, multiple?: VideoValue, offset?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulateKaleid',
        group: 'Warping',
        description:
            "As Hydra's `modulateKaleid`: mirrors `input` around the center into `sides` wedges, with the distance from the center pushed out by the modulator's red channel, so the kaleidoscope swells where the modulator is bright.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
            },
            { name: 'sides', description: 'Number of wedges (default 4)' },
        ],
        examples: [
            "$v.out($v.modulateKaleid($v.hsv($v.osc($v.ramp(), 6, $v.time)), $v.ramp('r'), 6))",
        ],
        declarations: [
            'modulateKaleid(input: VideoField, modulator: VideoField | VideoColor, sides?: VideoValue): VideoField;',
            'modulateKaleid(input: VideoColor, modulator: VideoField | VideoColor, sides?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulateHue',
        group: 'Warping',
        description:
            "As Hydra's `modulateHue`: pushes `input` by the differences between the modulator's color channels, green minus red across and blue minus green up, by `amount` pixels where a difference is full scale. A field has the same value in every channel, so it has no differences and does not move `input`.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
            },
            {
                name: 'amount',
                description:
                    'Pixels of push for a channel difference of full scale (default 50)',
            },
        ],
        examples: [
            "$v.out($v.modulateHue($v.hsv($v.osc($v.ramp(), 8)), $v.hsv($v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time)), 120))",
        ],
        declarations: [
            'modulateHue(input: VideoField, modulator: VideoField | VideoColor, amount?: VideoValue): VideoField;',
            'modulateHue(input: VideoColor, modulator: VideoField | VideoColor, amount?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulateRepeat',
        group: 'Warping',
        description:
            "As Hydra's `modulateRepeat`: tiles `input` `repeatX` by `repeatY` times with alternate rows and columns offset, the offsets growing with the modulator's red channel for x and green for y. 5 is a whole tile.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
            },
            { name: 'repeatX', description: 'Tiles across (default 3)' },
            { name: 'repeatY', description: 'Tiles up (default 3)' },
            {
                name: 'offsetX',
                description:
                    'Shift of alternate rows at a full red channel, with 5 a whole tile (default 2.5)',
            },
            {
                name: 'offsetY',
                description:
                    'Shift of alternate columns at a full green channel (default 2.5)',
            },
        ],
        examples: [
            "$v.out($v.modulateRepeat($v.hsv(3, 5, $v.shape($v.ramp(), $v.ramp('v'), 1.5)), $v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time), 4, 4, 2.5, 2.5))",
        ],
        declarations: [
            'modulateRepeat(input: VideoField, modulator: VideoField | VideoColor, repeatX?: VideoValue, repeatY?: VideoValue, offsetX?: VideoValue, offsetY?: VideoValue): VideoField;',
            'modulateRepeat(input: VideoColor, modulator: VideoField | VideoColor, repeatX?: VideoValue, repeatY?: VideoValue, offsetX?: VideoValue, offsetY?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulateRepeatX',
        group: 'Warping',
        description:
            "As Hydra's `modulateRepeatX`: tiles `input` `reps` times across, shifting alternate columns up by `offset` times the modulator's red channel.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
            },
            { name: 'reps', description: 'Tiles across (default 3)' },
            {
                name: 'offset',
                description:
                    'Shift at a full red channel, with 5 a whole tile (default 2.5)',
            },
        ],
        examples: [
            "$v.out($v.modulateRepeatX($v.hsv(3, 5, $v.shape($v.ramp(), $v.ramp('v'), 1.5)), $v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time), 4))",
        ],
        declarations: [
            'modulateRepeatX(input: VideoField, modulator: VideoField | VideoColor, reps?: VideoValue, offset?: VideoValue): VideoField;',
            'modulateRepeatX(input: VideoColor, modulator: VideoField | VideoColor, reps?: VideoValue, offset?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulateRepeatY',
        group: 'Warping',
        description:
            "As Hydra's `modulateRepeatY`: tiles `input` `reps` times up, shifting alternate rows right by `offset` times the modulator's red channel.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
            },
            { name: 'reps', description: 'Tiles up (default 3)' },
            {
                name: 'offset',
                description:
                    'Shift at a full red channel, with 5 a whole tile (default 2.5)',
            },
        ],
        examples: [
            "$v.out($v.modulateRepeatY($v.hsv(3, 5, $v.shape($v.ramp(), $v.ramp('v'), 1.5)), $v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time), 4))",
        ],
        declarations: [
            'modulateRepeatY(input: VideoField, modulator: VideoField | VideoColor, reps?: VideoValue, offset?: VideoValue): VideoField;',
            'modulateRepeatY(input: VideoColor, modulator: VideoField | VideoColor, reps?: VideoValue, offset?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulateScrollX',
        group: 'Warping',
        description:
            "As Hydra's `modulateScrollX`: scrolls `input` across by the modulator's red channel times `scroll`, plus `speed` of the frame every second, wrapping round the edges.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
            },
            {
                name: 'scroll',
                description:
                    'Scroll at a full red channel, with 5 the whole width (default 2.5)',
            },
            {
                name: 'speed',
                description:
                    'Scroll per second, with 5 the whole width (default 0)',
            },
        ],
        examples: [
            "$v.out($v.modulateScrollX($v.hsv($v.osc($v.ramp(), 8)), $v.ramp('v'), 2.5, 0.5))",
        ],
        declarations: [
            'modulateScrollX(input: VideoField, modulator: VideoField | VideoColor, scroll?: VideoValue, speed?: VideoValue): VideoField;',
            'modulateScrollX(input: VideoColor, modulator: VideoField | VideoColor, scroll?: VideoValue, speed?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'modulateScrollY',
        group: 'Warping',
        description:
            "As Hydra's `modulateScrollY`: scrolls `input` up by the modulator's red channel times `scroll`, plus `speed` of the frame every second, wrapping round the edges.",
        params: [
            {
                name: 'modulator',
                description:
                    'The signal that moves `input`; a color uses its channels, a field its one value',
            },
            {
                name: 'scroll',
                description:
                    'Scroll at a full red channel, with 5 the whole height (default 2.5)',
            },
            {
                name: 'speed',
                description:
                    'Scroll per second, with 5 the whole height (default 0)',
            },
        ],
        examples: [
            "$v.out($v.modulateScrollY($v.hsv($v.osc($v.ramp('v'), 8)), $v.ramp(), 2.5, 0.5))",
        ],
        declarations: [
            'modulateScrollY(input: VideoField, modulator: VideoField | VideoColor, scroll?: VideoValue, speed?: VideoValue): VideoField;',
            'modulateScrollY(input: VideoColor, modulator: VideoField | VideoColor, scroll?: VideoValue, speed?: VideoValue): VideoColor;',
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
            '$v.out($v.kaleid($v.hsv($v.osc($v.ramp(), 3, $v.time)), 6))',
            "$v.out($v.hsv($v.kaleid($v.noise($v.mult($v.ramp(), 5), $v.mult($v.ramp('v'), 5), $v.time), 8)))",
        ],
        declarations: [
            'kaleid(input: VideoField, sides?: VideoValue): VideoField;',
            'kaleid(input: VideoColor, sides?: VideoValue): VideoColor;',
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
        examples: [
            "$v.out($v.hsv($v.pixelate($v.noise($v.mult($v.ramp(), 4), $v.mult($v.ramp('v'), 4), $v.time), 24, 14)))",
        ],
        declarations: [
            'pixelate(input: VideoField, x?: VideoValue, y?: VideoValue): VideoField;',
            'pixelate(input: VideoColor, x?: VideoValue, y?: VideoValue): VideoColor;',
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
        examples: [
            "$v.out($v.repeat($v.hsv(3, 5, $v.shape($v.ramp(), $v.ramp('v'), 1.5)), 4, 3))",
        ],
        declarations: [
            'repeat(input: VideoField, x?: VideoValue, y?: VideoValue): VideoField;',
            'repeat(input: VideoColor, x?: VideoValue, y?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'hueShift',
        group: 'Color',
        description:
            'Turns every hue of a color by `amount` of a full circle, keeping its saturation and brightness.',
        params: [
            {
                name: 'amount',
                description:
                    'How far round the hue circle to turn, with 5 all the way round; 2.5 is the opposite hue (default 2.5)',
            },
        ],
        examples: [
            '$v.out($v.hueShift($v.hsv($v.ramp()), $v.osc($v.time, 0.5)))',
        ],
        declarations: [
            'hueShift(input: VideoSignal, amount?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'contrast',
        group: 'Color',
        description:
            "Scales each channel's distance from mid-gray by `amount`: above 5 pushes colors apart, below 5 pulls them toward gray, and 5 leaves them as they are.",
        params: [
            {
                name: 'amount',
                description: 'Contrast factor; 5 is no change (default 8)',
            },
        ],
        examples: [
            "$v.out($v.contrast($v.hsv($v.ramp(), 3, $v.ramp('v')), 15))",
        ],
        declarations: [
            'contrast(input: VideoSignal, amount?: VideoValue): VideoColor;',
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
                    'How dark each line gets at its darkest, 0 to 5 (default 2)',
            },
        ],
        examples: ['$v.out($v.scanlines($v.hsv($v.ramp()), 120, 2.5))'],
        declarations: [
            'scanlines(input: VideoSignal, count?: VideoValue, strength?: VideoValue): VideoColor;',
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
                description: 'How dark the corners get, 0 to 5 (default 3)',
            },
            {
                name: 'radius',
                description:
                    'Distance from the center, with 5 the whole frame height, where the darkening starts (default 1.5)',
            },
        ],
        examples: ['$v.out($v.vignette($v.hsv($v.ramp()), 4, 1))'],
        declarations: [
            'vignette(input: VideoSignal, strength?: VideoValue, radius?: VideoValue): VideoColor;',
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
                    'Largest change to a channel, 0 to 5 (default 0.5)',
            },
        ],
        examples: [
            '$v.out($v.grain($v.hsv($v.ramp(), 3, 3), 0.75))',
            '$v.hsv($v.ramp()).$.scanlines(180, 1.75).$.vignette(3.5).$.grain(0.4).out()',
        ],
        declarations: [
            'grain(input: VideoSignal, amount?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'channel',
        group: 'Color',
        description:
            "One channel of a color as a field: `'r'`, `'g'`, `'b'`, or `'luma'` for brightness (the default). A field is its own channel. A color's channels are also its `.r`, `.g` and `.b` properties.",
        params: [{ name: 'which', description: 'Which channel to take' }],
        examples: [
            "$v.out($v.colorize($v.channel($v.hsv($v.ramp()), 'r'), 0, 0))",
        ],
        declarations: [
            "channel(input: VideoSignal, which?: 'r' | 'g' | 'b' | 'luma'): VideoField;",
        ],
    },
    {
        name: 'swiz',
        group: 'Color',
        description:
            "Reorders the channels of a color. Each letter of `pattern` names the channel of `input` that fills red, green and blue in turn: `'gbr'` makes red from green, green from blue and blue from red, and `'rrr'` is the red channel as gray. A field is gray, so every pattern gives it back as a gray color.",
        params: [
            {
                name: 'pattern',
                description:
                    "Three of `r`, `g` and `b`, such as `'rrr'` or `'gbr'`",
            },
        ],
        examples: [
            "$v.out($v.swiz($v.hsv($v.ramp()), 'gbr'))",
            "$v.hsv($v.ramp()).$.swiz('bgr').out()",
        ],
        declarations: [
            'swiz(input: VideoSignal, pattern: string): VideoColor;',
        ],
    },
    {
        name: 'buffer',
        group: 'Memory',
        description:
            "A frame store that persists from one frame to the next, as Hydra's output buffers do. `write` stores a color; `read` returns what the buffer held on the previous frame, resampled through a zoom, rotation, shift and edge mode (see `$v.feedback`). Any number of signals can read a buffer, and buffers can read each other, which `$v.feedback`'s single loop cannot express. A buffer that is read must be written, and can be written once. A patch can use seven buffers and feedback loops together.",
        params: [],
        examples: [
            'const trail = $v.buffer(); $v.osc($v.ramp(), 8, $v.time).$.hsv().$.mult(1).$.add(trail.read({ zoom: 1.01, rotate: 0.01 }).$.mult(4.8)).write(trail).out()',
            "const a = $v.buffer(); const b = $v.buffer(); a.write($v.hsv($v.time, 5, $v.shape($v.ramp(), $v.ramp('v'), 0.4)).$.add(b.read({ rotate: 0.05 }).$.mult(4.75))); b.write(a.read({ zoom: 1.03 }).$.mult(4.5)); $v.out(a.read())",
        ],
        declarations: ['buffer(): VideoBuffer;'],
    },
    {
        name: 'out',
        group: 'Output',
        description:
            'Shows a color behind the code, and in the performance window when it is open. A field is shown as a gray picture. Last call wins. It returns its input unchanged, so it can sit in the middle of a chain.',
        params: [],
        examples: ['$v.out($v.colorize(5, 0, 0))'],
        declarations: [
            'out(input: VideoField): VideoField;',
            'out(input: VideoColor): VideoColor;',
        ],
    },
];

/** One method a video signal can be chained with. */
export interface VideoChainDoc {
    name: string;
    description: string;
    /** Declarations for every signal the method applies to; `{self}` is the signal's own type. */
    declarations?: string[];
    /** Declarations that replace the shared ones for a field or a color. */
    field?: string[];
    color?: string[];
    /** The kinds of signal the method applies to (default both). */
    on?: ('field' | 'color')[];
    /** A method of the signal itself that ends or taps a chain, not a member of `.$`. */
    direct?: boolean;
}

export const VIDEO_CHAIN_INTRO =
    'Video signals chain the way audio signals do. `.$` is a namespace of the `$v` functions that take a signal first, with the signal supplied for you: `x.$.rotate(0.5)` reads in signal-flow order and is `$v.warp(x, { rotate: 0.5 })`. `.$m` is the same with a leading `mix` argument that crossfades the signal against the result, 0 for the signal and 5 for the result: `x.$m.kaleid(2.5, 6)`. `.pipe(fn)` calls `fn(signal)`, and `.pipe(fn, array)` calls it once per element and returns the results as an array. `.pipeMix(fn, mix)` crossfades the signal against `fn(signal)`, half way (2.5) by default. `range(min, max)` is called directly on a field and maps its 0 to 5 volts onto `min` to `max`. `out`, `preview`, `toCV`, `write` and `range` end, tap or rescale a chain and are called directly on the signal. `rotate`, `scale` and `scroll` are shorthand for `warp`.';

export const VIDEO_CHAIN_EXAMPLES: string[] = [
    "$v.osc($v.ramp(), 10).$.modulate($v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time), 1.5).$.kaleid(6).$.hsv().out()",
    "$v.noise($v.mult($v.ramp(), 4), $v.mult($v.ramp('v'), 4), $v.time).$.rotate(0.5).$.pixelate(32, 18).$.hsv(4).preview().out()",
    '$v.osc($v.ramp(), 8).$.hsv().$m.kaleid(2.5, 5).out()',
    "$v.ramp().$.osc(8).$.mult($v.ramp('v').$.osc(4)).$.hsv().out()",
    '$v.osc($v.ramp(), 8).$.hsv().pipe((c) => c.$.kaleid(5).$.hueShift(1.5)).out()',
    '$v.osc($v.ramp(), 8).$.hsv().pipeMix((c) => c.$.invert(), $v.osc($v.time, 1)).out()',
];

export const VIDEO_CHAIN: VideoChainDoc[] = [
    {
        name: 'add',
        description: 'Sum with `b`, clipped to 0 to 5.',
        field: [
            'add(b: VideoValue): VideoField;',
            'add(b: VideoSignal): VideoColor;',
        ],
        color: ['add(b: VideoSignal): VideoColor;'],
    },
    {
        name: 'mult',
        description: 'Product with `b`.',
        field: [
            'mult(b: VideoValue): VideoField;',
            'mult(b: VideoSignal): VideoColor;',
        ],
        color: ['mult(b: VideoSignal): VideoColor;'],
    },
    {
        name: 'diff',
        description: 'Absolute difference from `b`.',
        field: [
            'diff(b: VideoValue): VideoField;',
            'diff(b: VideoSignal): VideoColor;',
        ],
        color: ['diff(b: VideoSignal): VideoColor;'],
    },
    {
        name: 'max',
        description: 'The larger of this and `b`.',
        field: [
            'max(b: VideoValue): VideoField;',
            'max(b: VideoSignal): VideoColor;',
        ],
        color: ['max(b: VideoSignal): VideoColor;'],
    },
    {
        name: 'min',
        description: 'The smaller of this and `b`.',
        field: [
            'min(b: VideoValue): VideoField;',
            'min(b: VideoSignal): VideoColor;',
        ],
        color: ['min(b: VideoSignal): VideoColor;'],
    },
    {
        name: 'mix',
        description: 'Crossfade from this to `b`.',
        field: [
            'mix(b: VideoValue, amount?: VideoValue): VideoField;',
            'mix(b: VideoSignal, amount?: VideoValue): VideoColor;',
        ],
        color: ['mix(b: VideoSignal, amount?: VideoValue): VideoColor;'],
    },
    {
        name: 'invert',
        description: 'The complement, 5 minus this.',
        declarations: ['invert(): {self};'],
    },
    {
        name: 'range',
        direct: true,
        description:
            "Maps this field's 0 to 5 volts onto `min` to `max` volts: `field.range(1, 3)` gives a value from 1 to 3, for an input in its own units such as a zoom factor or a count.",
        declarations: [
            'range(min?: VideoValue, max?: VideoValue): VideoField;',
        ],
        on: ['field'],
    },
    {
        name: 'posterize',
        description: 'Quantizes to `levels` steps.',
        declarations: ['posterize(levels?: VideoValue): VideoField;'],
        on: ['field'],
    },
    {
        name: 'write',
        direct: true,
        description:
            'Stores this in a buffer for the next frame to read, and returns it as a color.',
        declarations: ['write(buffer: VideoBuffer): VideoColor;'],
    },
    {
        name: 'wrap',
        description: 'Multiplies by `gain`, then keeps the fractional part.',
        declarations: ['wrap(gain?: VideoValue): VideoField;'],
        on: ['field'],
    },
    {
        name: 'fold',
        description:
            'Multiplies by `gain`, then reflects what passes 1 back down.',
        declarations: ['fold(gain?: VideoValue): VideoField;'],
        on: ['field'],
    },
    {
        name: 'osc',
        description:
            'Shapes this field into a repeating wave: `freq` cycles per unit of this field, offset by `phase`.',
        declarations: [
            "osc(\n    freq: VideoValue,\n    phase?: VideoValue,\n    config?: { shape?: 'sine' | 'triangle' | 'saw' | 'square' },\n): VideoField;",
        ],
        on: ['field'],
    },
    {
        name: 'comparator',
        description: 'Threshold with a soft edge.',
        declarations: [
            'comparator(threshold?: VideoValue, softness?: VideoValue): VideoField;',
        ],
        on: ['field'],
    },
    {
        name: 'hsv',
        description:
            'Uses this field as the hue of a color, so the field paints a rainbow. To color a mask instead, with a fixed hue and this field as the brightness, use `tint`.',
        declarations: [
            'hsv(saturation?: VideoValue, value?: VideoValue): VideoColor;',
        ],
        on: ['field'],
    },
    {
        name: 'tint',
        description:
            'Colors this field: it becomes the brightness of a color with the given hue and saturation, so a mask turns into a shape of that color.',
        declarations: [
            'tint(hue?: VideoValue, saturation?: VideoValue): VideoColor;',
        ],
        on: ['field'],
    },
    {
        name: 'procAmp',
        description: 'Saturation, then gain and bias.',
        declarations: [
            'procAmp(gain?: VideoValue, bias?: VideoValue, saturation?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'hueShift',
        description:
            'Turns every hue of this color round the hue circle by `amount`, 5 being all the way round.',
        declarations: ['hueShift(amount?: VideoValue): VideoColor;'],
    },
    {
        name: 'contrast',
        description: "Scales this color's distance from mid-gray by `amount`.",
        declarations: ['contrast(amount?: VideoValue): VideoColor;'],
    },
    {
        name: 'blur',
        description: 'Averages this over a disk of `radius`.',
        declarations: ['blur(radius?: VideoValue): {self};'],
    },
    {
        name: 'bloom',
        description:
            'Adds a blurred copy of this back on top of it, so bright areas glow.',
        declarations: [
            'bloom(radius?: VideoValue, amount?: VideoValue): {self};',
        ],
    },
    {
        name: 'edges',
        description: 'Brightness of the steepest change around each pixel.',
        declarations: ['edges(amount?: VideoValue): VideoField;'],
    },
    {
        name: 'scanlines',
        description: 'Darkens evenly spaced horizontal lines of this color.',
        declarations: [
            'scanlines(count?: VideoValue, strength?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'vignette',
        description: 'Darkens this color toward the corners of the frame.',
        declarations: [
            'vignette(strength?: VideoValue, radius?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'grain',
        description: 'Adds film grain to this color, redrawn every frame.',
        declarations: ['grain(amount?: VideoValue): VideoColor;'],
    },
    {
        name: 'channel',
        description:
            'One channel of this as a field; a field is its own channel.',
        declarations: [
            "channel(which?: 'r' | 'g' | 'b' | 'luma'): VideoField;",
        ],
    },
    {
        name: 'swiz',
        description:
            "Reorders the channels of this color: each letter of `pattern` names the channel that fills red, green and blue in turn, so `'gbr'` shifts them round and `'rrr'` is the red channel as gray. A field is gray, so it comes back as a gray color.",
        declarations: ['swiz(pattern: string): VideoColor;'],
    },
    {
        name: 'frameDelay',
        description:
            'This as it was `frames` frames ago, as a color; black until then.',
        declarations: ['frameDelay(frames?: number): VideoColor;'],
    },
    {
        name: 'key',
        description:
            'Shows this where `mask` is 5 and `background` where it is 0.',
        declarations: [
            'key(background: VideoSignal, mask: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'warp',
        description: 'Zooms, turns and shifts everything this draws.',
        declarations: [
            'warp(config?: { zoom?: VideoValue; rotate?: VideoValue; shiftX?: VideoValue; shiftY?: VideoValue }): {self};',
        ],
    },
    {
        name: 'rotate',
        description:
            'Turns everything this draws by `amount`, with 5 a full turn; positive is clockwise.',
        declarations: ['rotate(amount: VideoValue): {self};'],
    },
    {
        name: 'scale',
        description: 'Zooms everything this draws by `zoom` about the center.',
        declarations: ['scale(zoom: VideoValue): {self};'],
    },
    {
        name: 'scroll',
        description:
            'Shifts everything this draws right by `x` and up by `y`, with 5 the whole frame.',
        declarations: ['scroll(x?: VideoValue, y?: VideoValue): {self};'],
    },
    {
        name: 'displace',
        description: 'Reads this at positions pushed by `dx` and `dy`.',
        declarations: [
            'displace(dx: VideoValue, dy?: VideoValue, amount?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulate',
        description: 'Pushes this around by another signal.',
        declarations: [
            'modulate(modulator: VideoField | VideoColor, amount?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulateScale',
        description:
            'Scales this about the center by `offset + multiple * channel` of a modulator.',
        declarations: [
            'modulateScale(modulator: VideoField | VideoColor, multiple?: VideoValue, offset?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulateRotate',
        description:
            'Turns this about the center by `offset + multiple * red` of a modulator.',
        declarations: [
            'modulateRotate(modulator: VideoField | VideoColor, multiple?: VideoValue, offset?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulatePixelate',
        description:
            'Holds this constant over a grid of `offset + multiple * channel` cells.',
        declarations: [
            'modulatePixelate(modulator: VideoField | VideoColor, multiple?: VideoValue, offset?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulateKaleid',
        description:
            'Mirrors this into `sides` wedges, pushed outward by a modulator.',
        declarations: [
            'modulateKaleid(modulator: VideoField | VideoColor, sides?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulateHue',
        description:
            "Pushes this by the differences between a modulator's color channels.",
        declarations: [
            'modulateHue(modulator: VideoField | VideoColor, amount?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulateRepeat',
        description:
            'Tiles this with alternate rows and columns offset by a modulator.',
        declarations: [
            'modulateRepeat(modulator: VideoField | VideoColor, repeatX?: VideoValue, repeatY?: VideoValue, offsetX?: VideoValue, offsetY?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulateRepeatX',
        description:
            'Tiles this across, shifting alternate columns by a modulator.',
        declarations: [
            'modulateRepeatX(modulator: VideoField | VideoColor, reps?: VideoValue, offset?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulateRepeatY',
        description: 'Tiles this up, shifting alternate rows by a modulator.',
        declarations: [
            'modulateRepeatY(modulator: VideoField | VideoColor, reps?: VideoValue, offset?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulateScrollX',
        description: 'Scrolls this across by a modulator, wrapping.',
        declarations: [
            'modulateScrollX(modulator: VideoField | VideoColor, scroll?: VideoValue, speed?: VideoValue): {self};',
        ],
    },
    {
        name: 'modulateScrollY',
        description: 'Scrolls this up by a modulator, wrapping.',
        declarations: [
            'modulateScrollY(modulator: VideoField | VideoColor, scroll?: VideoValue, speed?: VideoValue): {self};',
        ],
    },
    {
        name: 'kaleid',
        description: 'Mirrors this around the center into `sides` wedges.',
        declarations: ['kaleid(sides?: VideoValue): {self};'],
    },
    {
        name: 'pixelate',
        description: 'Holds this constant across a grid of `x` by `y` cells.',
        declarations: ['pixelate(x?: VideoValue, y?: VideoValue): {self};'],
    },
    {
        name: 'repeat',
        description: 'Tiles this `x` by `y` times.',
        declarations: ['repeat(x?: VideoValue, y?: VideoValue): {self};'],
    },
    {
        name: 'preview',
        direct: true,
        description: 'Shows this in the editor and returns it.',
        declarations: [
            "preview(config?: { view?: 'image' | 'waveform' | 'vectorscope' }): {self};",
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
            'Shows this behind the code, and in the performance window when it is open; a field is shown as a gray picture. Returns this unchanged, so the chain can go on.',
        declarations: ['out(): {self};'],
    },
];
