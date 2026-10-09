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
    'Color',
    'Math',
    'Shaping',
    'Compositing',
    'Memory',
    'Output',
];

export const VIDEO_INTRO = {
    description:
        'Video synthesis. Patches build a graph of fields that is drawn per pixel in the performance window, which opens when a patch calls `$v.out`. Video signals cannot be connected to audio inputs.\n\nSliders, buttons and audio signals can be passed anywhere a field is accepted, and the picture follows them live. Audio signals are sampled about 60 times a second and used in volts as the audio graph produces them, so scale them to the range an input expects, for example `.range(0, 1)`. Pass one channel; a polyphonic signal is rejected.',
    examples: [
        "$v.out($v.hsv($slider('Hue', 0.3, 0, 1), 1, $slider('Level', 1, 0, 1)))",
        "$v.out($v.hsv($sine('0.2hz').range(0, 1), 1, $v.shape($v.ramp(), $v.ramp('v'), $sine('1hz').range(0.1, 0.4))))",
    ],
};

export const VIDEO_DOCS: VideoDoc[] = [
    {
        name: 'time',
        group: 'Generators',
        description:
            'Seconds since the performance window started rendering. Use it as a phase to animate an oscillator.',
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
                    "`'h'` horizontal 0..1 (default), `'v'` vertical 0..1, `'d'` diagonal, `'r'` distance from the center (0.5 at the top and bottom edges), `'a'` angle around the center, 0..1 once around",
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
            "$v.out($v.colorize($v.ramp(), $v.ramp('v'), 0.5))",
            "$v.out($v.hsv($v.osc($v.ramp('r'), 6, $v.time)))",
            "$v.out($v.hsv($v.ramp('a', { rotate: $v.osc($v.time, 0.1) })))",
            "$v.out($v.colorize($v.osc($v.ramp('h', { rotate: 0.125, zoom: 2 }), 8), 0.2, 0.5))",
        ],
        declarations: [
            "ramp(\n    axis?: 'h' | 'v' | 'd' | 'r' | 'a',\n    config?: {\n        zoom?: VideoValue;\n        rotate?: VideoValue;\n        shiftX?: VideoValue;\n        shiftY?: VideoValue;\n    },\n): VideoField;",
        ],
    },
    {
        name: 'osc',
        group: 'Generators',
        description:
            'Periodic shaper: `freq` cycles per unit of `input`, offset by `phase` cycles.',
        params: [
            { name: 'input', description: 'Field to shape, usually a ramp' },
            {
                name: 'freq',
                description: 'Cycles across the full range of `input`',
            },
            { name: 'phase', description: 'Offset in cycles (default 0)' },
            {
                name: 'config.shape',
                description:
                    "`'sine'` (default), `'triangle'`, `'saw'` or `'square'`",
            },
        ],
        examples: [
            '$v.out($v.colorize($v.osc($v.ramp(), 8), 0, 0))',
            "$v.out($v.colorize($v.osc($v.ramp(), 3, $v.time, { shape: 'saw' }), $v.osc($v.ramp('v'), 5), 0.3))",
        ],
        declarations: [
            "osc(\n    input: VideoValue,\n    freq: VideoValue,\n    phase?: VideoValue,\n    config?: { shape?: 'sine' | 'triangle' | 'saw' | 'square' },\n): VideoField;",
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
                    'A single-channel audio signal, such as an oscillator or an envelope',
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
            "$v.out($v.hsv(0.35, 1, $v.invert($v.comparator($v.diff($v.ramp('v'), $v.add(0.5, $v.mult($v.fromAudio($sine('110hz')), 0.08))), 0.01, 0.01))))",
            "$v.out($v.hsv($v.fromAudio($saw('55hz'), $v.ramp('r'), { samples: 1024 }), 1, 1))",
        ],
        declarations: [
            'fromAudio(signal: ModuleOutput | Collection | CollectionWithRange, position?: VideoValue, config?: { samples?: number; trigger?: boolean }): VideoField;',
        ],
    },
    {
        name: 'noise',
        group: 'Generators',
        description:
            'Smooth value noise between 0 and 1. One unit of any coordinate is one cell of the noise, so scaling the coordinates sets the grain; `z` moves through the noise, which animates it when fed `$v.time`.',
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
            "$v.out($v.hsv($v.noise($v.mult($v.ramp(), 6), $v.mult($v.ramp('v'), 6), $v.mult($v.time, 0.5))))",
            "$v.out($v.hsv(0.6, 1, $v.comparator($v.noise($v.mult($v.ramp('r'), 10), $v.mult($v.ramp('a'), 6), $v.time), 0.5, 0.05)))",
        ],
        declarations: [
            'noise(x: VideoValue, y: VideoValue, z?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'shape',
        group: 'Generators',
        description:
            '1 inside a shape and 0 outside, with a soft edge. Pass ramps for x and y to center it on the frame; offset them to move it. Circles stay round at any window aspect.',
        params: [
            {
                name: 'x',
                description: 'Horizontal position field; 0.5 is the center',
            },
            {
                name: 'y',
                description: 'Vertical position field; 0.5 is the center',
            },
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
        ],
        examples: [
            "$v.out($v.hsv(0.6, 1, $v.shape($v.ramp(), $v.ramp('v'), 0.3, 0.05)))",
            "$v.out($v.hsv($v.time, 1, $v.shape($v.ramp(), $v.ramp('v'), 0.2, 0, { shape: 'diamond' })))",
        ],
        declarations: [
            "shape(\n    x: VideoValue,\n    y: VideoValue,\n    size?: VideoValue,\n    softness?: VideoValue,\n    config?: { shape?: 'circle' | 'box' | 'diamond' },\n): VideoField;",
        ],
    },
    {
        name: 'colorize',
        group: 'Color',
        description:
            'Combines three fields into a color. Each channel is clipped to 0..1.',
        params: [],
        examples: ["$v.out($v.colorize($v.ramp(), $v.ramp('v'), 1))"],
        declarations: [
            'colorize(r: VideoValue, g: VideoValue, b: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'hsv',
        group: 'Color',
        description:
            'Color from hue, saturation and value. Hue wraps every 1.0; saturation and value are clipped to 0..1.',
        params: [],
        examples: [
            '$v.out($v.hsv($v.ramp(), 1, 1))',
            "$v.out($v.hsv($v.osc($v.ramp(), 2, $v.time), 0.8, $v.ramp('v')))",
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
            'procAmp(\n    input: VideoSignal,\n    gain?: VideoValue,\n    bias?: VideoValue,\n    saturation?: VideoValue,\n): VideoColor;',
        ],
    },
    {
        name: 'add',
        group: 'Math',
        description: 'Sum, clipped to 0..1. Colors add per channel.',
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
            'Product. Multiplying by a field darkens; colors multiply per channel.',
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
            "$v.out($v.hsv(0.1, 1, $v.max($v.shape($v.ramp(), $v.ramp('v'), 0.2), $v.shape($v.add($v.ramp(), 0.2), $v.ramp('v'), 0.2))))",
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
            "$v.out($v.hsv(0.5, 1, $v.min($v.shape($v.ramp(), $v.ramp('v'), 0.3), $v.shape($v.add($v.ramp(), 0.2), $v.ramp('v'), 0.3))))",
        ],
        declarations: [
            'min(a: VideoValue, b: VideoValue): VideoField;',
            'min(a: VideoSignal, b: VideoSignal): VideoColor;',
        ],
    },
    {
        name: 'invert',
        group: 'Math',
        description: 'Complement, 1 minus the input.',
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
        description: 'Crossfade from `a` (amount 0) to `b` (amount 1).',
        params: [
            {
                name: 'amount',
                description: 'Mix position, clipped to 0..1 (default 0.5)',
            },
        ],
        examples: [
            "$v.out($v.mix($v.hsv($v.ramp()), $v.hsv($v.ramp('v')), $v.osc($v.time, 0.25)))",
        ],
        declarations: [
            'mix(a: VideoValue, b: VideoValue, amount?: VideoValue): VideoField;',
            'mix(a: VideoSignal, b: VideoSignal, amount?: VideoValue): VideoColor;',
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
            'wrap(input: VideoValue, gain?: VideoValue): VideoField;',
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
            'fold(input: VideoValue, gain?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'comparator',
        group: 'Shaping',
        description:
            'Threshold: 0 below `threshold`, 1 above, with a linear ramp `softness` wide centered on it (a hard edge when 0).',
        params: [],
        examples: [
            '$v.out($v.hsv(0.1, 1, $v.comparator($v.osc($v.ramp(), 4), 0.5, 0.1)))',
        ],
        declarations: [
            'comparator(\n    input: VideoValue,\n    threshold?: VideoValue,\n    softness?: VideoValue,\n): VideoField;',
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
            'posterize(input: VideoValue, levels?: VideoValue): VideoField;',
        ],
    },
    {
        name: 'key',
        group: 'Compositing',
        description: 'Shows `fg` where `mask` is 1 and `bg` where it is 0.',
        params: [],
        examples: [
            "$v.out($v.key($v.hsv(0.9), $v.hsv($v.ramp(), 1, 0.5), $v.shape($v.ramp(), $v.ramp('v'), 0.3)))",
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
            "$v.out($v.feedback((prev) => $v.mix($v.hsv($v.time, 1, $v.shape($v.ramp(), $v.ramp('v'), 0.1)), prev, 0.9), { zoom: 1.02, rotate: 0.004 }))",
            "$v.out($v.feedback((prev) => $v.add($v.hsv($v.osc($v.time, 0.2), 1, $v.shape($v.add($v.ramp(), -0.2), $v.ramp('v'), 0.05)), $v.mult(prev, 0.96)), { rotate: 0.01 }))",
        ],
        declarations: [
            "feedback(\n    update: (prev: VideoColor) => VideoColor,\n    config?: {\n        zoom?: VideoValue;\n        rotate?: VideoValue;\n        shiftX?: VideoValue;\n        shiftY?: VideoValue;\n        edge?: 'clamp' | 'repeat' | 'mirror';\n    },\n): VideoColor;",
        ],
    },
    {
        name: 'preview',
        group: 'Output',
        description:
            'Shows a signal in the editor, in a panel under this call, and returns it unchanged so a preview can sit inside an expression. The panel is drawn from the same frame the performance window shows, feedback included, so the performance window must be open.',
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
            'Averages a region of a signal each frame into an audio control signal between 0 and 1, so the picture can modulate the sound. A color contributes its brightness. The value is read from the performance window, about 30 times a second, so the performance window must be open. Scale the result with `.range(min, max)` like any ranged signal.',
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
            "$sine($v.toCV($v.shape($v.ramp(), $v.ramp('v'), 0.3), { size: 0.1 }).range(110, 440)).out()",
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
            "$v.out($v.hsv($v.warp($v.noise($v.mult($v.ramp(), 4), $v.mult($v.ramp('v'), 4)), { rotate: $v.mult($v.time, 0.05), zoom: 2 })))",
            "$v.out($v.warp($v.hsv($v.shape($v.ramp(), $v.ramp('v'), 0.15)), { shiftX: $v.osc($v.time, 0.25), rotate: 0.1 }))",
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
            "$v.out($v.hsv($v.displace($v.osc($v.ramp(), 8), $v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time), 0.5, 0.2)))",
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
                    'Largest push as a fraction of the frame (default 0.1)',
            },
        ],
        examples: [
            "$v.out($v.hsv($v.modulate($v.osc($v.ramp(), 10), $v.noise($v.mult($v.ramp(), 3), $v.mult($v.ramp('v'), 3), $v.time), 0.3)))",
            "$v.out($v.modulate($v.hsv($v.ramp('r')), $v.hsv($v.noise($v.mult($v.ramp(), 4), $v.mult($v.ramp('v'), 4), 0)), 0.2))",
        ],
        declarations: [
            'modulate(input: VideoField, modulator: VideoField | VideoColor, amount?: VideoValue): VideoField;',
            'modulate(input: VideoColor, modulator: VideoField | VideoColor, amount?: VideoValue): VideoColor;',
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
            "$v.out($v.repeat($v.hsv(0.6, 1, $v.shape($v.ramp(), $v.ramp('v'), 0.3)), 4, 3))",
        ],
        declarations: [
            'repeat(input: VideoField, x?: VideoValue, y?: VideoValue): VideoField;',
            'repeat(input: VideoColor, x?: VideoValue, y?: VideoValue): VideoColor;',
        ],
    },
    {
        name: 'channel',
        group: 'Color',
        description:
            "One channel of a color as a field: `'r'`, `'g'`, `'b'`, or `'luma'` for brightness (the default).",
        params: [{ name: 'which', description: 'Which channel to take' }],
        examples: [
            "$v.out($v.colorize($v.channel($v.hsv($v.ramp()), 'r'), 0, 0))",
        ],
        declarations: [
            "channel(input: VideoColor, which?: 'r' | 'g' | 'b' | 'luma'): VideoField;",
        ],
    },
    {
        name: 'out',
        group: 'Output',
        description: 'Shows a color in the performance window. Last call wins.',
        params: [],
        examples: ['$v.out($v.colorize(1, 0, 0))'],
        declarations: ['out(input: VideoColor): void;'],
    },
];
