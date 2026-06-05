/**
 * Shared type documentation for DSL types.
 * Used by both TypeScript lib generator (JSDoc) and HelpWindow (rendered docs).
 */

export interface TypeMethod {
    name: string;
    signature: string;
    description: string;
    example?: string;
}

export interface TypeDocumentation {
    name: string;
    description: string;
    definition?: string;
    examples: string[];
    seeAlso: string[];
    methods?: TypeMethod[];
}

/**
 * All DSL type names that should be linkified in documentation.
 */
export const DSL_TYPE_NAMES = [
    'Signal',
    'Poly<Signal>',
    'Mono<Signal>',
    'ModuleOutput',
    'ModuleOutputWithRange',
    'Collection',
    'CollectionWithRange',
    'Note',
    'HZ',
    'MidiNote',
    'Scale',
    'StereoOutOptions',
] as const;

export type DslTypeName = (typeof DSL_TYPE_NAMES)[number];

/**
 * Comprehensive documentation for all DSL types.
 */
export const TYPE_DOCS: Record<DslTypeName, TypeDocumentation> = {
    Collection: {
        definition:
            'interface extends Iterable<ModuleOutput> { length: number; [index]: ModuleOutput; ... }',
        description:
            'A collection of ModuleOutput instances with chainable DSP methods. ' +
            'Created with the $() helper function. Supports iteration, indexing, and spreading. ' +
            'Methods operate on all outputs in the collection.',
        examples: [
            '$c(osc1, osc2, osc3).amplitude(0.5).out()  // Apply amplitude to all, send to output',
            'const voices = $c(osc1, osc2, osc3)',
            'for (const v of voices) { ... }      // Iterate over outputs',
            '[...voices]                          // Spread to array',
            'voices[0]                            // Index access',
        ],
        methods: [
            {
                name: 'amplitude',
                signature: 'amplitude(factor: Poly<Signal>): Collection',
                description:
                    'Scale all signals in the collection by a linear factor (5 = unity, 2.5 = half, 10 = 2x). For perceptual (audio-taper) volume control, use gain() instead.',
                example: '$c(osc1, osc2).amplitude(2.5)',
            },
            {
                name: 'amp',
                signature: 'amp(factor: Poly<Signal>): Collection',
                description:
                    'Alias for amplitude(). Scale all signals in the collection by a factor.',
                example: '$c(osc1, osc2).amp(0.5)',
            },
            {
                name: 'shift',
                signature: 'shift(offset: Poly<Signal>): Collection',
                description:
                    'Add a DC offset to all signals in the collection.',
                example: '$c(lfo1, lfo2).shift(2.5)',
            },
            {
                name: 'gain',
                signature: 'gain(level: Poly<Signal>): Collection',
                description:
                    'Scale all signals with a perceptual (audio taper) curve (5 = unity, 0 = silence). For linear amplitude scaling, use amplitude() instead.',
                example: '$c(osc1, osc2).gain(2.5)',
            },
            {
                name: 'exp',
                signature: 'exp(factor?: Poly<Signal>): Collection',
                description:
                    'Apply a power curve to all signals in the collection. Default exponent is 3.',
                example: '$c(lfo1, lfo2).exp(2)',
            },
            {
                name: 'scope',
                signature:
                    'scope(config?: { msPerFrame?: number; triggerThreshold?: number; range?: [number, number] }): this',
                description:
                    'Add scope visualization for the first output in the collection.',
                example: '$c(osc1, osc2).scope().out()',
            },
            {
                name: 'out',
                signature: 'out(options?: StereoOutOptions): this',
                description:
                    'Send all outputs to speakers as stereo, summed together.',
                example: '$c(osc1, osc2, osc3).out()',
            },
            {
                name: 'outMono',
                signature:
                    'outMono(channel?: number, gain?: Poly<Signal>): this',
                description:
                    'Send all outputs to a single speaker channel as mono, summed together.',
                example: '$c(osc1, osc2).outMono(0, 0.3)',
            },
            {
                name: 'range',
                signature:
                    'range(outMin: Poly<Signal>, outMax: Poly<Signal>, inMin: Poly<Signal>, inMax: Poly<Signal>): Collection',
                description:
                    'Remap all outputs from input range to output range. Requires explicit input min/max.',
                example: '$c(lfo1, lfo2).range(0, 1, -5, 5)',
            },
            {
                name: 'pipe',
                signature: 'pipe<T>(pipeFn: (self: this) => T): T',
                description:
                    'Pass this collection through a transform function and return the result. ' +
                    'Enables inline transforms and reusable signal-processing helpers.',
                example: '$c(osc1, osc2).pipe(all => all.amplitude(0.5)).out()',
            },
            {
                name: 'pipe',
                signature:
                    'pipe<T>(pipeFn: (self: this, item: E) => T, array: E[]): Collection',
                description:
                    'Call pipeFn once for every element in the provided array, ' +
                    'collecting all results into a Collection.',
                example:
                    "$c(osc1, osc2).pipe((col, cutoff) => $lpf(col, cutoff), ['200hz', '800hz', '3200hz']).out()",
            },
            {
                name: 'pipeMix',
                signature:
                    'pipeMix(pipeFn: (self: this) => ModuleOutput | Collection, options?: { mode?: "sum" | "average" | "max" | "min"; gain?: Poly<Signal> }): Collection',
                description:
                    'Pipe this collection through a transform, then mix the original and transformed signals together using a $mix module. ' +
                    'The callback receives this collection and returns a second signal; both are passed as inputs to $mix.',
                example: "$c(osc1, osc2).pipeMix(s => $lpf(s, '1000hz')).out()",
            },
        ],
        name: 'Collection',
        seeAlso: ['CollectionWithRange', 'ModuleOutput', 'Poly<Signal>'],
    },

    CollectionWithRange: {
        definition: 'interface extends Iterable<ModuleOutputWithRange> { ... }',
        description:
            'A collection of ModuleOutputWithRange instances. ' +
            'Created with the $r() helper function. Like Collection, but the range() method uses stored min/max values.',
        examples: [
            '$r(lfo1, lfo2).range(0, 5).out()     // Remap using stored ranges',
            '$r(...seq.gates).range(0, 1)        // Spread and remap gates',
        ],
        methods: [
            {
                name: 'range',
                signature:
                    'range(outMin: Poly<Signal>, outMax: Poly<Signal>, inMin?: Poly<Signal>, inMax?: Poly<Signal>): CollectionWithRange',
                description:
                    'Remap all outputs from their native ranges to a new range. ' +
                    "Uses each output's stored minValue/maxValue. " +
                    'Optional inMin/inMax override the declared input bounds per-bound (nullish, so an explicit 0 is honored). ' +
                    'Returns a CollectionWithRange whose own range tracks the remapped bounds, so further .range(...) calls chain off it.',
                example:
                    '$r(lfo1, lfo2).range(200, 2000)            // input range inferred\n' +
                    '$r(lfo1, lfo2).range(0, 1, -2, 2)          // override input bounds with -2..2',
            },
        ],
        name: 'CollectionWithRange',
        seeAlso: ['Collection', 'ModuleOutputWithRange'],
    },

    HZ: {
        definition: '`${number}hz` | `${number}Hz`',
        description:
            'A frequency string specifying a value in Hertz. ' +
            'Case-insensitive suffix "hz". Converted to 1V/oct voltage internally.',
        examples: [
            '"440hz"   // A4 concert pitch',
            '"261.63Hz" // Middle C',
            '"1000hz"  // 1 kHz',
        ],
        name: 'HZ',
        seeAlso: ['Signal', 'Note'],
    },

    MidiNote: {
        definition: '`${number}m`',
        description:
            'A MIDI note number string. MIDI note 60 is middle C (C4). ' +
            'Converted to 1V/oct voltage internally.',
        examples: [
            '"60m"  // Middle C (C4)',
            '"69m"  // A4 (440 Hz)',
            '"36m"  // C2',
        ],
        name: 'MidiNote',
        seeAlso: ['Signal', 'Note'],
    },

    ModuleOutput: {
        definition:
            'interface { moduleId: string; portName: string; channel: number; ... }',
        description:
            'A single output from a module, representing a mono signal connection. ' +
            'ModuleOutputs are chainable - methods like amplitude(), shift(), and out() return the same output for fluent API usage. ' +
            'Every module factory returns either a ModuleOutput or a Collection of outputs.',
        examples: [
            'const osc = $sine("c4")',
            'osc.amplitude(0.5).out()           // Chain methods',
            'osc.scope().out()             // Add visualization',
            "$lpf(osc, 'c3', { q: 4 })     // Use as input to another module",
        ],
        methods: [
            {
                name: 'amplitude',
                signature: 'amplitude(factor: Poly<Signal>): ModuleOutput',
                description:
                    'Scale the signal by a linear factor (5 = unity, 2.5 = half, 10 = 2x). Creates a $scaleAndShift module internally. For perceptual (audio-taper) volume control, use gain() instead.',
                example: 'osc.amplitude(2.5)  // Half amplitude',
            },
            {
                name: 'amp',
                signature: 'amp(factor: Poly<Signal>): ModuleOutput',
                description:
                    'Alias for amplitude(). Scale the signal by a factor.',
                example: 'osc.amp(2.5)  // Half amplitude',
            },
            {
                name: 'shift',
                signature: 'shift(offset: Poly<Signal>): ModuleOutput',
                description:
                    'Add a DC offset to the signal. Creates a $scaleAndShift module internally.',
                example: 'lfo.shift(2.5)  // Shift LFO to 0-5V range',
            },
            {
                name: 'gain',
                signature: 'gain(level: Poly<Signal>): ModuleOutput',
                description:
                    'Scale the signal with a perceptual (audio taper) curve (5 = unity, 0 = silence). Chains $curve and $scaleAndShift internally with exponent 3. For linear amplitude scaling, use amplitude() instead.',
                example: 'osc.gain(2.5)',
            },
            {
                name: 'exp',
                signature: 'exp(factor?: Poly<Signal>): ModuleOutput',
                description:
                    'Apply a power curve to the signal. Creates a $curve module internally. Default exponent is 3.',
                example: 'lfo.exp(2)  // Quadratic curve',
            },
            {
                name: 'scope',
                signature:
                    'scope(config?: { msPerFrame?: number; triggerThreshold?: number; range?: [number, number] }): this',
                description:
                    'Add an oscilloscope visualization for this output. The scope appears as an overlay in the editor.',
                example:
                    'osc.scope({ msPerFrame: 100, range: [-10, 10] }).out()',
            },
            {
                name: 'out',
                signature: 'out(options?: StereoOutOptions): this',
                description:
                    'Send this output to the speakers as stereo audio. Left plays on baseChannel, right on baseChannel+1.',
                example: 'osc.out({ baseChannel:0, gain: 2.5, pan: -2 })',
            },
            {
                name: 'outMono',
                signature:
                    'outMono(channel?: number, gain?: Poly<Signal>): this',
                description:
                    'Send this output to a single speaker channel as mono audio.',
                example: 'lfo.outMono(2, 0.3)',
            },
            {
                name: 'pipe',
                signature: 'pipe<T>(pipeFn: (self: this) => T): T',
                description:
                    'Pass this output through a transform function and return the result. ' +
                    'Enables inline transforms and reusable signal-processing helpers.',
                example: "$saw('c4').pipe(s => s.amplitude(0.5)).out()",
            },
            {
                name: 'pipe',
                signature:
                    'pipe<T>(pipeFn: (self: this, item: E) => T, array: E[]): Collection',
                description:
                    'Call pipeFn once for every element in the provided array, ' +
                    'collecting all results into a Collection. ' +
                    'Useful for generating a family of voices that each vary by one parameter.',
                example:
                    '// Create 2 groups of 3 voices with different cutoff values\n' +
                    "$sine(['C4', 'E4', 'G4']).pipe((s, cut) => $lfp(s, cut), ['440hz', '880hz']).out()",
            },
            {
                name: 'pipeMix',
                signature:
                    'pipeMix(pipeFn: (self: this) => ModuleOutput | Collection, options?: { mode?: "sum" | "average" | "max" | "min"; gain?: Poly<Signal> }): Collection',
                description:
                    'Pipe this output through a transform, then mix the original and transformed signals together using a $mix module. ' +
                    'The callback receives this output and returns a second signal; both are passed as inputs to $mix.',
                example: "$saw('c4').pipeMix(s => $lpf(s, '1000hz')).out()",
            },
            {
                name: 'range',
                signature:
                    'range(outMin: Poly<Signal>, outMax: Poly<Signal>, inMin: Poly<Signal>, inMax: Poly<Signal>): ModuleOutput',
                description:
                    'Remap this output from an explicit input range to a new output range. Creates a $remap module internally.',
                example: "$sine('c4').range(0, 1, -5, 5)",
            },
        ],
        name: 'ModuleOutput',
        seeAlso: ['ModuleOutputWithRange', 'Collection', 'Signal'],
    },

    ModuleOutputWithRange: {
        definition:
            'interface extends ModuleOutput { minValue: number; maxValue: number; range(...): CollectionWithRange }',
        description:
            'An extension of ModuleOutput that knows its output value range (minValue, maxValue). ' +
            'Typically returned by LFOs, envelopes, and other modulation sources. ' +
            'The range() method uses the stored min/max for automatic scaling.',
        examples: [
            "const lfo = $sine('1hz').range(0, 5)              // LFO outputs 0 to +5",
        ],
        methods: [
            {
                name: 'range',
                signature:
                    'range(outMin: Poly<Signal>, outMax: Poly<Signal>, inMin?: Poly<Signal>, inMax?: Poly<Signal>): CollectionWithRange',
                description:
                    'Remap the output from its native range (minValue, maxValue) to a new range (outMin, outMax). ' +
                    'Unlike Collection.range(), this uses the stored min/max values automatically. ' +
                    'Optional inMin/inMax override the declared input bounds per-bound (nullish, so an explicit 0 is honored). ' +
                    'Returns a CollectionWithRange whose own range tracks the remapped bounds, so further .range(...) calls chain off it.',
                example:
                    'lfo.range(note("c3"), note("c5"))  // input range inferred from minValue/maxValue\n' +
                    'lfo.range(0, 1, -2, 2)             // override the input bounds with -2..2',
            },
        ],
        name: 'ModuleOutputWithRange',
        seeAlso: ['ModuleOutput', 'CollectionWithRange'],
    },

    'Mono<Signal>': {
        definition: 'Signal | Signal[] | Iterable<ModuleOutput>',
        description:
            'A signal input that accepts polyphonic connections but sums all channels down to a single mono value. ' +
            'Structurally identical to Poly<Signal>, but signals that the module will not produce per-voice output from this parameter. ' +
            'Used for control parameters like tempo, stereo width, or math variables where a single combined value is needed.',
        examples: [
            '$clockDivider(clock.trigger, 4)                  // Clock signal summed to mono',
            '$stereoMix(osc, { width: lfo })                  // Width control summed to mono',
            '$math("x + y", { x: osc1, y: osc2 })            // Variables summed to single values',
        ],
        name: 'Mono<Signal>',
        seeAlso: ['Signal', 'Poly<Signal>', 'ModuleOutput'],
    },

    Note: {
        definition: '`${NoteName}${Accidental}${Octave}`',
        description:
            'A musical note string in scientific pitch notation. ' +
            'Consists of a note name (A-G or a-g), optional accidental (#/b), and optional octave number. ' +
            'If octave is omitted, defaults to octave 4.',
        examples: [
            '"C4"   // Middle C',
            '"A#3"  // A sharp in octave 3',
            '"Bb5"  // B flat in octave 5',
            '"G"    // G4 (octave 4 is default)',
            '"c#"    // C#4 (octave 4 is default)',
        ],
        name: 'Note',
        seeAlso: ['Signal', 'HZ', 'MidiNote'],
    },

    'Poly<Signal>': {
        definition: 'Signal | Signal[] | Iterable<ModuleOutput>',
        description:
            'A potentially multi-channel signal. Can be an array of Signals for polyphonic patches, ' +
            'or an iterable of ModuleOutputs. When used as input to a module, arrays are expanded to create multiple voices.',
        examples: [
            'filter.lpf(["C3", "E3", "G3"], { cutoff: 1000 })  // 3-voice chord',
            'osc.saw([...seq.pitch])                           // Spread sequencer outputs',
            'mix.add(osc1.out, osc2.out, osc3.out)             // Multiple ModuleOutputs',
        ],
        name: 'Poly<Signal>',
        seeAlso: ['Signal', 'Mono<Signal>', 'ModuleOutput', 'Collection'],
    },

    Scale: {
        definition: '`${number}s(${Note}:${Mode})`',
        description:
            'A scale pattern string for generating multiple pitches. ' +
            'Format: "{count}s({root}:{mode})" where count is the number of notes, ' +
            'root is the root note, and mode is the scale type.',
        examples: [
            '"4s(C:major)"     // 4 notes of C major scale',
            '"8s(A:minor)"     // 8 notes of A minor scale',
            '"3s(G:dorian)"    // 3 notes of G dorian mode',
            '"5s(E:pentatonic minor)"  // E minor pentatonic',
        ],
        name: 'Scale',
        seeAlso: ['Signal', 'Note'],
    },

    Signal: {
        definition: 'number | Note | HZ | MidiNote | Scale | ModuleOutput',
        description:
            'A single-channel audio signal value. This is the fundamental type for all audio connections in the modular system. ' +
            'Signals follow the 1V/octave convention where 0V corresponds to C4 (~261.63 Hz).',
        examples: [
            'sine("C4")           // Note string - converted to 1V/oct',
            'sine(440)            // Number - constant voltage',
            'sine("440hz")        // Hz string - converted to voltage',
            'sine("60m")          // MIDI note 60 (middle C)',
            'sine(lfo.out)        // ModuleOutput from another module',
            'sine("4s(C:major)")  // Scale pattern',
        ],
        name: 'Signal',
        seeAlso: [
            'Poly<Signal>',
            'Mono<Signal>',
            'ModuleOutput',
            'Note',
            'HZ',
            'MidiNote',
            'Scale',
        ],
    },

    StereoOutOptions: {
        definition:
            'interface { baseChannel?: number; gain?: Poly<Signal>; pan?: Poly<Signal>; width?: Signal }',
        description:
            'Options for stereo output routing via the out() method. ' +
            'Controls base channel, gain, panning, and stereo width.',
        examples: [
            "$sine('c').out({ baseChannel: 4 })      // Output of channels 4 and 5",
            "$sine('c').out({ gain: 2.5 })           // 50% gain",
            "$sine('c').out({ pan: -2.5 })           // Pan left",
            "$sine('c').out({ width: 5 })            // Full stereo spread",
            "$sine('c').out({ gain: $perc($pulse('8hz')), pan: $sine('1hz') })  // Modulated",
        ],
        name: 'StereoOutOptions',
        seeAlso: ['ModuleOutput', 'Collection', 'Poly<Signal>'],
    },
};

export interface GlobalFunctionDoc {
    name: string;
    signature: string;
    description: string;
    params?: { name: string; type: string; description: string }[];
    returns?: string;
    examples: string[];
    group: string;
}

/**
 * Documentation for all DSL global functions and helpers.
 */
export const GLOBAL_DOCS: GlobalFunctionDoc[] = [
    // ---- Helpers ----
    {
        description:
            'Convert a frequency in Hertz to a 1V/octave voltage value.',
        examples: ['$hz(440)    // A4', '$hz(261.63) // ~C4'],
        group: 'Helpers',
        name: '$hz',
        params: [
            {
                name: 'frequency',
                type: 'number',
                description: 'Frequency in Hz',
            },
        ],
        returns: 'Voltage value usable as a Signal',
        signature: '$hz(frequency: number): number',
    },
    {
        description: 'Convert a note name string to a 1V/octave voltage value.',
        examples: ['$note("C4")  // Middle C', '$note("A4")  // 440 Hz'],
        group: 'Helpers',
        name: '$note',
        params: [
            {
                name: 'noteName',
                type: 'string',
                description: 'Note name like "C4", "A#3", "Bb5"',
            },
        ],
        returns: 'Voltage value usable as a Signal',
        signature: '$note(noteName: string): number',
    },
    {
        description:
            'Create a Collection from one or more ModuleOutputs. Collections support chainable DSP methods, indexing, and spreading.',
        examples: [
            '$c(osc1, osc2).amplitude(0.5).out()',
            '$c(osc1, osc2)[0]  // index access',
        ],
        group: 'Helpers',
        name: '$c',
        signature:
            '$c(...args: (ModuleOutput | Iterable<ModuleOutput>)[]): Collection',
    },
    {
        description:
            'Create a CollectionWithRange from ranged outputs. The range() method uses stored min/max values.',
        examples: [
            '$r(lfo1, lfo2).range(0, 5).out()',
            '$r(...seq.gates).range(0, 1)',
        ],
        group: 'Helpers',
        name: '$r',
        signature:
            '$r(...args: (ModuleOutputWithRange | Iterable<ModuleOutputWithRange>)[]): CollectionWithRange',
    },
    {
        description:
            'Compute the Cartesian product of the given arrays. Returns every possible combination of one element from each array.',
        examples: [
            "$cartesian([220, 440], ['sine', 'saw'])\n// → [[220,'sine'],[220,'saw'],[440,'sine'],[440,'saw']]",
            "$cartesian([1, 2], ['a', 'b']).pipe(\n  (osc, [freq, shape]) => $oscillator({ freq, shape }).out(),\n)",
        ],
        group: 'Helpers',
        name: '$cartesian',
        signature:
            '$cartesian<A extends unknown[][]>(...arrays: A): ElementsOf<A>[]',
    },
    // ---- Global Settings ----
    {
        description: 'Set the global tempo for the root clock.',
        examples: ['$setTempo(120)  // 120 BPM', '$setTempo(140)  // 140 BPM'],
        group: 'Global Settings',
        name: '$setTempo',
        params: [
            { name: 'tempo', type: 'number', description: 'Tempo in BPM' },
        ],
        signature: '$setTempo(tempo: number): void',
    },
    {
        description:
            'Set the time signature for the root clock. Both values must be positive integers.',
        examples: [
            '$setTimeSignature(4, 4)  // 4/4 (default)',
            '$setTimeSignature(3, 4)  // 3/4 waltz',
            '$setTimeSignature(7, 8)  // 7/8 asymmetric',
        ],
        group: 'Global Settings',
        name: '$setTimeSignature',
        params: [
            {
                name: 'numerator',
                type: 'number',
                description: 'Beats per bar (e.g. 3, 4, 6, 7)',
            },
            {
                name: 'denominator',
                type: 'number',
                description:
                    'Beat value (e.g. 4 for quarter note, 8 for eighth note)',
            },
        ],
        signature:
            '$setTimeSignature(numerator: number, denominator: number): void',
    },
    {
        description:
            'Set the global output gain applied to the final mix. 2.5 is the default (50%); 5.0 is unity gain.',
        examples: [
            '$setOutputGain(2.5) // 50% gain (default)',
            '$setOutputGain(5.0) // unity gain',
            "$setOutputGain($sine('1hz')) // modulated gain",
        ],
        group: 'Global Settings',
        name: '$setOutputGain',
        params: [
            {
                name: 'gain',
                type: 'Mono<Signal>',
                description: 'Gain level (2.5 default, 5.0 unity)',
            },
        ],
        signature: '$setOutputGain(gain: Mono<Signal>): void',
    },
    // ---- Controls -----
    {
        description:
            'Create a UI slider bound to a signal module. The slider appears in the Control panel. Dragging it updates the audio engine and the source code value in real time.',
        examples: [
            'const vol = $slider("Volume", 0.5, 0, 1);\n$sine(440).amplitude(vol).out();',
            'const cutoff = $slider("Cutoff", 1000, 100, 8000);\n$saw(440).pipe(s => $lpf(s, cutoff)).out();',
        ],
        group: 'Controls',
        name: '$slider',
        params: [
            {
                name: 'label',
                type: 'string',
                description: 'Display label (must be a string literal)',
            },
            {
                name: 'value',
                type: 'number',
                description: 'Initial value (must be a numeric literal)',
            },
            { name: 'min', type: 'number', description: 'Minimum value' },
            { name: 'max', type: 'number', description: 'Maximum value' },
        ],
        returns: 'CollectionWithRange carrying the current slider value, with a static [min, max] range so .range(outMin, outMax) infers the input bounds',
        signature:
            '$slider(label: string, value: number, min: number, max: number): CollectionWithRange',
    },
    // ---- Advanced ----
    {
        description:
            'Create placeholder signals that can be assigned later. Useful for feedback loops.',
        examples: [
            'const feedback = $deferred();\nconst delayed = $delay(osc.out, feedback[0]);\nfeedback.set(delayed);',
        ],
        group: 'Advanced',
        name: '$deferred',
        params: [
            {
                name: 'channels',
                type: 'number',
                description: 'Number of deferred outputs (1-16, default 1)',
            },
        ],
        returns: 'DeferredCollection',
        signature: '$deferred(channels?: number): DeferredCollection',
    },

    {
        description:
            'Create a send-return bus. Signals are routed to the bus via `.send(bus, gain)` on any ModuleOutput or Collection. ' +
            'The callback receives a Collection that is the mix of all sends, allowing effects or further routing to be applied.',
        examples: [
            'const fx = $bus((mixed) => $reverb(mixed).out());\n$saw(440).send(fx, 0.6);\n$sine(220).send(fx, 0.4);',
            '// Multiple sends at different gain levels\nconst verb = $bus((mixed) => mixed.gain(0.8).out());\nvoices.send(verb, 0.5);',
        ],
        group: 'Advanced',
        name: '$bus',
        params: [
            {
                name: 'cb',
                type: '(mixed: Collection) => unknown',
                description:
                    'Called at patch finalization with the mixed sends. Call .out() or return a signal.',
            },
        ],
        returns: 'Bus handle passed to .send()',
        signature: '$bus(cb: (mixed: Collection) => unknown): Bus',
    },
    {
        description:
            'Set a custom processor applied to the final mix just before the global output gain. ' +
            'The callback receives the fully mixed Collection and should return a processed signal.',
        examples: [
            `$setEndOfChainCb((mix) => $lpf(mix, '2000hz'));`,
            '$setEndOfChainCb((mix) => mix.scope());',
        ],
        group: 'Advanced',
        name: '$setEndOfChainCb',
        params: [
            {
                name: 'cb',
                type: '(mixed: Collection) => ModuleOutput | Collection | CollectionWithRange',
                description: 'Transform applied to the final mix',
            },
        ],
        signature:
            '$setEndOfChainCb(cb: (mixed: Collection) => ModuleOutput | Collection | CollectionWithRange): void',
    },
];

/**
 * Check if a string is a known DSL type name.
 */
export function isDslType(name: string): name is DslTypeName {
    return DSL_TYPE_NAMES.includes(name as DslTypeName);
}

/**
 * Get documentation for a DSL type by name.
 */
export function getTypeDoc(name: string): TypeDocumentation | undefined {
    if (isDslType(name)) {
        return TYPE_DOCS[name];
    }
    return undefined;
}
