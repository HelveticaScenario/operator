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
        name: 'out',
        group: 'Output',
        description: 'Shows a color in the performance window. Last call wins.',
        params: [],
        examples: ['$v.out($v.colorize(1, 0, 0))'],
        declarations: ['out(input: VideoColor): void;'],
    },
];
