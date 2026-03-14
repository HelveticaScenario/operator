import { ModuleSchema, getReservedOutputNames } from '@modular/core';
import {
    JSONSchema,
    resolveRef,
    schemaToTypeExpr,
    getEnumVariants,
} from '../../shared/dsl/schemaTypeResolver';

const BASE_LIB_SOURCE = `
/** The **\`console\`** object provides access to the debugging console (e.g., the Web console in Firefox). */
/**
 * The **\`console\`** object provides access to the debugging console (e.g., the Web console in Firefox).
 *
 * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console)
 */
interface Console {
    /**
     * The **\`console.assert()\`** static method writes an error message to the console if the assertion is false.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/assert_static)
     */
    assert(condition?: boolean, ...data: any[]): void;
    /**
     * The **\`console.clear()\`** static method clears the console if possible.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/clear_static)
     */
    clear(): void;
    /**
     * The **\`console.count()\`** static method logs the number of times that this particular call to \`count()\` has been called.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/count_static)
     */
    count(label?: string): void;
    /**
     * The **\`console.countReset()\`** static method resets counter used with console/count_static.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/countReset_static)
     */
    countReset(label?: string): void;
    /**
     * The **\`console.debug()\`** static method outputs a message to the console at the 'debug' log level.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/debug_static)
     */
    debug(...data: any[]): void;
    /**
     * The **\`console.dir()\`** static method displays a list of the properties of the specified JavaScript object.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/dir_static)
     */
    dir(item?: any, options?: any): void;
    /**
     * The **\`console.dirxml()\`** static method displays an interactive tree of the descendant elements of the specified XML/HTML element.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/dirxml_static)
     */
    dirxml(...data: any[]): void;
    /**
     * The **\`console.error()\`** static method outputs a message to the console at the 'error' log level.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/error_static)
     */
    error(...data: any[]): void;
    /**
     * The **\`console.group()\`** static method creates a new inline group in the Web console log, causing any subsequent console messages to be indented by an additional level, until console/groupEnd_static is called.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/group_static)
     */
    group(...data: any[]): void;
    /**
     * The **\`console.groupCollapsed()\`** static method creates a new inline group in the console.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/groupCollapsed_static)
     */
    groupCollapsed(...data: any[]): void;
    /**
     * The **\`console.groupEnd()\`** static method exits the current inline group in the console.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/groupEnd_static)
     */
    groupEnd(): void;
    /**
     * The **\`console.info()\`** static method outputs a message to the console at the 'info' log level.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/info_static)
     */
    info(...data: any[]): void;
    /**
     * The **\`console.log()\`** static method outputs a message to the console.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/log_static)
     */
    log(...data: any[]): void;
    /**
     * The **\`console.table()\`** static method displays tabular data as a table.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/table_static)
     */
    table(tabularData?: any, properties?: string[]): void;
    /**
     * The **\`console.time()\`** static method starts a timer you can use to track how long an operation takes.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/time_static)
     */
    time(label?: string): void;
    /**
     * The **\`console.timeEnd()\`** static method stops a timer that was previously started by calling console/time_static.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/timeEnd_static)
     */
    timeEnd(label?: string): void;
    /**
     * The **\`console.timeLog()\`** static method logs the current value of a timer that was previously started by calling console/time_static.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/timeLog_static)
     */
    timeLog(label?: string, ...data: any[]): void;
    timeStamp(label?: string): void;
    /**
     * The **\`console.trace()\`** static method outputs a stack trace to the console.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/trace_static)
     */
    trace(...data: any[]): void;
    /**
     * The **\`console.warn()\`** static method outputs a warning message to the console at the 'warning' log level.
     *
     * [MDN Reference](https://developer.mozilla.org/docs/Web/API/console/warn_static)
     */
    warn(...data: any[]): void;
}

var console: Console;

interface Array<T> {
  /**
   * Pipe this array through a transform function.
   *
   * Passes \`this\` to \`pipeFn\` and returns the result, enabling inline
   * functional transforms and method chaining on any array.
   *
   * @param pipeFn - A function that receives this array and returns a transformed value
   * @returns The return value of \`pipeFn\`
   *
   * @example
   * // Pipe an array of outputs
   * [osc1, osc2, osc3].pipe(all => $mix(all)).out()
   */
  pipe<U>(this: this, pipeFn: (self: this) => U): U;
}

type NoteNames = "a" | "A" | "b" | "B" | "c" | "C" | "d" | "D" | "e" | "E" | "f" | "F" | "g" | "G"
type Accidental = "" | "#" | "b"
type Note = \`\${NoteNames}\${Accidental}\${number | ''}\`

type HZ = \`\${number}hz\` | \`\${number}Hz\`

type MidiNote = \`\${number}m\`

type CaseVariants<T extends string> = 
  | Lowercase<T>
  | Uppercase<T>
  | Capitalize<T>;

type ModeString =
  // Ionian (Major)
  | \`M \${string}\`
  | "M"
  | \`\${string}\${CaseVariants<"maj">}\${string}\`
  | \`\${string}\${CaseVariants<"major">}\${string}\`
  | \`\${string}\${CaseVariants<"ionian">}\${string}\`
  
  // Harmonic Minor
  | \`\${string}\${CaseVariants<"har">} \${CaseVariants<"minor">}\${string}\`
  | \`\${string}\${CaseVariants<"harmonic">}\${CaseVariants<"minor">}\${string}\`
  | \`\${string}\${CaseVariants<"harmonic">} \${CaseVariants<"minor">}\${string}\`
  
  // Melodic Minor
  | \`\${string}\${CaseVariants<"mel">} \${CaseVariants<"minor">}\${string}\`
  | \`\${string}\${CaseVariants<"melodic">}\${CaseVariants<"minor">}\${string}\`
  | \`\${string}\${CaseVariants<"melodic">} \${CaseVariants<"minor">}\${string}\`
  
  // Pentatonic Major
  | \`\${string}\${CaseVariants<"pentatonic">} \${CaseVariants<"major">}\${string}\`
  | \`\${string}\${CaseVariants<"pentatonic">} \${CaseVariants<"maj">}\${string}\`
  | \`\${string}\${CaseVariants<"pent">} \${CaseVariants<"maj">}\${string}\`
  | \`\${string}\${CaseVariants<"pent">} \${CaseVariants<"major">}\${string}\`
  
  // Pentatonic Minor
  | \`\${string}\${CaseVariants<"pentatonic">} \${CaseVariants<"minor">}\${string}\`
  | \`\${string}\${CaseVariants<"pentatonic">} \${CaseVariants<"min">}\${string}\`
  | \`\${string}\${CaseVariants<"pent">} \${CaseVariants<"min">}\${string}\`
  | \`\${string}\${CaseVariants<"pent">} \${CaseVariants<"minor">}\${string}\`
  
  // Blues
  | \`\${string}\${CaseVariants<"blues">}\${string}\`
  
  // Chromatic
  | \`\${string}\${CaseVariants<"chromatic">}\${string}\`
  
  // Whole Tone
  | \`\${string}\${CaseVariants<"whole">} \${CaseVariants<"tone">}\${string}\`
  | \`\${string}\${CaseVariants<"whole">}\${CaseVariants<"tone">}\${string}\`
  
  // Aeolian (Minor)
  | \`m \${string}\`
  | "m"
  | \`\${string}\${CaseVariants<"min">}\${string}\`
  | \`\${string}\${CaseVariants<"minor">}\${string}\`
  | \`\${string}\${CaseVariants<"aeolian">}\${string}\`
  
  // Dorian (start of string)
  | \`\${CaseVariants<"dorian">}\${string}\`
  
  // Locrian (start of string)
  | \`\${CaseVariants<"locrian">}\${string}\`
  
  // Mixolydian (start of string)
  | \`\${CaseVariants<"mixolydian">}\${string}\`
  
  // Phrygian (start of string)
  | \`\${CaseVariants<"phrygian">}\${string}\`
  
  // Lydian (start of string)
  | \`\${CaseVariants<"lydian">}\${string}\`;

/**
 * A scale pattern string for generating multiple pitches.
 * Format: "{count}s({root}:{mode})"
 * @example "4s(C:major)" - 4 notes of C major scale
 * @example "8s(A:minor)" - 8 notes of A minor scale
 * @see {@link Signal}
 * @see {@link Note}
 */
type Scale = \`\${number}s(\${Note}:\${ModeString})\`

type OrArray<T> = T | T[];

/**
 * Extracts the element types from a tuple of arrays.
 * Used as the return type of {@link $cartesian} to enable typed destructuring.
 * @example
 * type T = ElementsOf<[number[], string[]]>; // [number, string]
 */
type ElementsOf<T extends unknown[][]> = { [K in keyof T]: T[K] extends (infer E)[] ? E : never };

/**
 * A single-channel audio signal value. The fundamental type for all audio connections.
 * 
 * Signals follow the 1V/octave convention where 0V = C4 (~261.63 Hz).
 * 
 * Can be one of:
 * - A **number** (constant voltage)
 * - A **{@link Note}** string like \`"C4"\` or \`"A#3"\`
 * - A **{@link HZ}** string like \`"440hz"\`
 * - A **{@link MidiNote}** string like \`"60m"\`
 * - A **{@link Scale}** pattern like \`"4s(C:major)"\`
 * - A **{@link ModuleOutput}** from another module
 * 
 * @example sine("C4")        // Note string
 * @example sine(440)         // Number
 * @example sine("440hz")     // Hz string
 * @example sine(lfo.out)     // ModuleOutput
 * @see {@link Poly<Signal>} - for multi-channel signals
 * @see {@link ModuleOutput} - for module connections
 */
type Signal = number | Note | HZ | MidiNote | Scale | ModuleOutput;

/**
 * A potentially multi-channel signal for polyphonic patches.
 * 
 * Can be:
 * - A single {@link Signal}
 * - An array of {@link Signal}s (creates multiple voices)
 * - An iterable of {@link ModuleOutput}s
 * 
 * @example filter.lpf(["C3", "E3", "G3"]) // 3-voice chord
 * @example osc.saw([...seq.pitch])        // Spread sequencer outputs
 * @see {@link Signal} - for single-channel signals
 * @see {@link Collection} - for grouping outputs
 */
type Poly<T extends Signal = Signal> = OrArray<T> | Iterable<ModuleOutput>;


/**
 * A signal input that sums all channels to a single mono value.
 * Structurally identical to {@link Poly}, but signals that the module
 * combines all voices into one control signal rather than preserving polyphony.
 *
 * @example $clock(120)                    // Constant tempo
 * @example $stereoMix(osc, { width: lfo }) // Width summed to mono
 * @see {@link Poly} - for polyphonic signals that preserve per-voice data
 * @see {@link Signal} - for single-channel signals
 */
type Mono<T extends Signal = Signal> = OrArray<T> | Iterable<ModuleOutput>;

/**
 * Options for stereo output routing via the out() method.
 * @see {@link ModuleOutput.out}
 * @see {@link Collection.out}
 */
interface StereoOutOptions {
  /** Output gain. If set, a util.scaleAndShift module is added after the stereo mix */
  gain?: Poly<Signal>;
  /** Pan position (-5 = left, 0 = center, +5 = right). Default 0 */
  pan?: Poly<Signal>;
  /** Stereo width/spread (0 = no spread, 5 = full spread). Default 0 */
  width?: Mono<Signal>;
}

/**
 * A single output from a module, representing a mono signal connection.
 * 
 * ModuleOutputs are chainable - methods like amplitude(), shift(), and out() 
 * return the same output for fluent API usage.
 * 
 * @example
 * const osc = osc.sine("C4")
 * osc.amplitude(0.5).out()           // Chain methods
 * osc.scope().out()             // Add visualization
 * filter.lpf(osc, { q: 4 })     // Use as input
 * 
 * @see {@link ModuleOutputWithRange} - for outputs with known value ranges
 * @see {@link Collection} - for grouping multiple outputs
 * @see {@link Signal} - ModuleOutput is a valid Signal
 */
interface ModuleOutput {
  /** The unique identifier of the module this output belongs to */
  readonly moduleId: string;
  /** The name of the output port */
  readonly portName: string;
  /** The channel index for polyphonic outputs */
  readonly channel: number;
  
  /**
   * Scale the signal by a factor. Creates a util.scaleAndShift module internally.
   * @param factor - Scale factor as {@link Poly<Signal>}
   * @returns The scaled {@link Collection} for chaining
    * @example osc.amplitude(0.5)  // Half amplitude
     */
   amplitude(factor: Poly<Signal>): Collection;

   /** Alias for {@link amplitude} */
   amp(factor: Poly<Signal>): Collection;
  
  /**
   * Add a DC offset to the signal. Creates a util.scaleAndShift module internally.
   * @param offset - Offset value as {@link Poly<Signal>}
   * @returns The shifted {@link Collection} for chaining
   * @example lfo.shift(2.5)  // Shift to 0-5V range
   */
  shift(offset: Poly<Signal>): Collection;

  /**
   * Scale the signal by a factor with a perceptual (audio taper) curve.
   * Chains \\$curve → \\$scaleAndShift with exponent 3.
   * @param level - Amplitude level as {@link Poly<Signal>}
   * @returns The scaled {@link Collection} for chaining
   * @example osc.gain(2.5)  // Perceptual half volume
   */
  gain(level: Poly<Signal>): Collection;

  /**
   * Apply a power curve to this signal. Creates a \\$curve module internally.
   * @param factor - Exponent for the curve (default 3)
   * @returns The curved {@link Collection} for chaining
   * @example lfo.exp(2)  // Quadratic curve
   */
  exp(factor?: Poly<Signal>): Collection;
  
  /**
   * Add scope visualization for this output.
   * The scope appears as an overlay in the editor.
   * @param config - Scope configuration options
   * @param config.msPerFrame - Time window in milliseconds (default 500)
   * @param config.triggerThreshold - Trigger threshold in volts (optional)
   * @param config.triggerWaitToRender - Whether the scope should wait to render until the buffer fills (default true). Only applicable if triggerThreshold is set.
   * @param config.range - Voltage range for display as [min, max] tuple (default [-5, 5])
   * @example osc.scope({ msPerFrame: 100, range: [-10, 10] }).out()
   */
  scope(config?: { msPerFrame?: number; triggerThreshold?: number; triggerWaitToRender?: boolean; range?: [number, number] }): this;
  
  /**
   * Send this output to speakers as stereo.
   * @param baseChannel - Base output channel (0-15, default 0). Left plays on baseChannel, right on baseChannel+1
   * @param options - Stereo output options ({@link StereoOutOptions})
   * @example osc.out(0, { gain: 2.5, pan: -2 })
   */
  out(baseChannel?: number, options?: StereoOutOptions): this;
  
  /**
   * Send this output to speakers as mono.
   * @param channel - Output channel (0-15, default 0)
    * @param gain - Output gain as {@link Poly<Signal>} (optional)
    * @example lfo.outMono(2, 0.3)
    */
   outMono(channel?: number, gain?: Poly<Signal>): this;

  /**
   * Pipe this output through a transform function.
   *
   * Passes \`this\` to \`pipeFn\` and returns the result, enabling inline
   * functional transforms and reusable signal-processing helpers.
   *
   * @param pipeFn - A function that receives this output and returns a transformed value
   * @returns The return value of \`pipeFn\`
   *
   * @example
   * // Inline transform
   * $sine('a').pipe(s => s.amplitude(0.5).shift(1))
   *
   * @example
   * // Reusable helper
   * const tremolo = (c) => c.amplitude($sine('10hz').range(4, 5))
   * $saw('c').pipe(tremolo).out()
   */
  pipe<T>(pipeFn: (self: this) => T): T;
  /**
   * Pipe this output through a transform for each element of an array.
   * Returns a {@link Collection} containing one output per element.
   *
   * @param pipeFn - A function that receives this output and one element from the array
   * @param array - An array whose elements are passed to \`pipeFn\` one by one
   * @returns A {@link Collection} with one item per element
   *
   * @example
   * // 6 outputs
   * $sine(['C4', 'E4', 'G4']).pipe(
   *   (s, cut) => $lph(s, cut),
   *   ['440hz', '880hz'],
   * ).out()
   */
  pipe<T extends ModuleOutput | Iterable<ModuleOutput>, E>(
    pipeFn: (self: this, item: E) => T,
    array: E[]
  ): Collection;

  /**
   * Pipe this output through a transform, then mix the original and transformed
   * signals together using a \\$mix module.
   *
   * @param pipeFn - A function that receives this output and returns a signal to mix with the original
   * @param mix - Optional crossfade as {@link Poly<Signal>}. 0 for only original, 5 for only transformed. Default is 2.5 for equal mix.
   * @returns A Collection from the \\$mix output
   *
   * @example
   * // Mix original with a filtered version
   * $saw('c4').pipeMix(s => $lpf(s, '1000hz')).out()
   *
   * @example
   * // Mix with custom balance
   * $saw('c4').pipeMix(s => $lpf(s, '1000hz'), 1.0).out()
   */
  pipeMix(pipeFn: (self: this) => ModuleOutput | Collection, mix?: Poly<Signal> ): Collection;

  /**
   * Remap this output from an explicit input range to a new output range.
   * Creates a $remap module internally.
   * @param outMin - New minimum as {@link Poly<Signal>}
   * @param outMax - New maximum as {@link Poly<Signal>}
   * @param inMin - Input minimum as {@link Poly<Signal>}
   * @param inMax - Input maximum as {@link Poly<Signal>}
   * @returns A {@link ModuleOutput} with the remapped signal
   * @example $sine('c4').range(0, 1, -5, 5)
   */
  range(outMin: Poly<Signal>, outMax: Poly<Signal>, inMin: Poly<Signal>, inMax: Poly<Signal>): ModuleOutput;
}

/**
 * DeferredModuleOutput is a placeholder for a signal that will be assigned later.
 * Useful for feedback loops and forward references in the DSL.
 * Supports the same chainable methods as ModuleOutput (amplitude, shift, scope, out, outMono).
 */
interface DeferredModuleOutput extends ModuleOutput {
  /**
   * Set the actual signal this deferred output should resolve to.
   * @param signal - The signal to resolve to (number, string, or ModuleOutput)
   */
  set(signal: Signal): void;
}

/**
 * A {@link ModuleOutput} that knows its output value range (minValue, maxValue).
 * 
 * Typically returned by LFOs, envelopes, and other modulation sources.
 * The range() method uses the stored min/max for automatic scaling.
 * 
 * @example
 * const lfo = lfo.sine(2)              // Outputs -5 to +5
 * lfo.range(200, 2000)                 // Remap to 200-2000
 * env.adsr({ attack: 0.1 }).range(0, 1)
 * 
 * @see {@link ModuleOutput} - base interface
 * @see {@link CollectionWithRange} - for collections of ranged outputs
 */
interface ModuleOutputWithRange extends ModuleOutput {
  /** The minimum value this output produces */
  readonly minValue: number;
  /** The maximum value this output produces */
  readonly maxValue: number;
  
  /**
   * Remap the output from its native range to a new range.
   * Uses the stored minValue/maxValue automatically.
   * @param outMin - New minimum as {@link Poly<Signal>}
   * @param outMax - New maximum as {@link Poly<Signal>}
   * @returns A {@link ModuleOutput} with the remapped signal
   * @example lfo.range(note("C3"), note("C5"))
   */
  range(outMin: Poly<Signal>, outMax: Poly<Signal>): ModuleOutput;
}


class BaseCollection<T extends ModuleOutput> implements Iterable<T> {
  /** Number of outputs in the collection */
  readonly length: number;
  /** Index access to individual elements */
  readonly [index: number]: T;
  [Symbol.iterator](): Iterator<T>;

  /**
   * Scale all signals by a factor.
   * @param factor - Scale factor as {@link Poly<Signal>}
     * @see {@link ModuleOutput.amplitude}
     */
   amplitude(factor: Poly<Signal>): Collection;

   /** Alias for {@link amplitude} */
   amp(factor: Poly<Signal>): Collection;

  /**
   * Add DC offset to all signals.
   * @param offset - Offset as {@link Poly<Signal>}
   * @see {@link ModuleOutput.shift}
   */
  shift(offset: Poly<Signal>): Collection;

  /**
   * Scale all signals by a factor with a perceptual (audio taper) curve.
   * @param level - Amplitude level as {@link Poly<Signal>}
   * @see {@link ModuleOutput.gain}
   */
  gain(level: Poly<Signal>): Collection;

  /**
   * Apply a power curve to all signals. Creates a \\$curve module internally.
   * @param factor - Exponent for the curve (default 3)
   * @see {@link ModuleOutput.exp}
   */
  exp(factor?: Poly<Signal>): Collection;

  /**
   * Add scope visualization for the first output in the collection.
   * @param config - Scope configuration options
   * @param config.msPerFrame - Time window in milliseconds (default 500)
   * @param config.triggerThreshold - Trigger threshold in volts (optional)
   * @param config.triggerWaitToRender - Whether the scope should wait to render until the buffer fills (default true). Only applicable if triggerThreshold is set.
   * @param config.range - Voltage range for display as [min, max] tuple (default [-5, 5])
   */
  scope(config?: { msPerFrame?: number; triggerThreshold?: number; triggerWaitToRender?: boolean; range?: [number, number] }): this;

  /**
   * Send all outputs to speakers as stereo, summed together.
   * @param baseChannel - Base output channel (0-14, default 0)
   * @param options - Stereo output options ({@link StereoOutOptions})
   */
  out(baseChannel?: number, options?: StereoOutOptions): this;

  /**
   * Send all outputs to speakers as mono, summed together.
   * @param channel - Output channel (0-15, default 0)
    * @param gain - Output gain as {@link Poly<Signal>} (optional)
    */
   outMono(channel?: number, gain?: Poly<Signal>): this;


  /**
   * Remap all outputs from input range to output range.
   * Requires explicit input min/max values.
   * @param inMin - Input minimum as {@link Poly<Signal>}
   * @param inMax - Input maximum as {@link Poly<Signal>}
   * @param outMin - Output minimum as {@link Poly<Signal>}
   * @param outMax - Output maximum as {@link Poly<Signal>}
   * @see {@link CollectionWithRange.range} - for automatic input range
   */
  range(outMin: Poly<Signal>, outMax: Poly<Signal>, inMin: Poly<Signal>, inMax: Poly<Signal>): Collection;

  /**
   * Pipe this collection through a transform function.
   *
   * Passes \`this\` to \`pipeFn\` and returns the result, enabling inline
   * functional transforms and reusable signal-processing helpers.
   *
   * @param pipeFn - A function that receives this collection and returns a transformed value
   * @returns The return value of \`pipeFn\`
   *
   * @example
   * // Inline transform on a collection
   * $(osc1, osc2).pipe(all => all.amplitude(2.5)).out()
   *
   * @example
   * // Reusable helper applied to a collection
   * const tremolo = (c) => c.amplitude($sine('10hz').range(4, 5))
   * $r($saw('220hz'), $saw('221hz')).pipe(tremolo).out()
   */
  pipe<T>(pipeFn: (self: this) => T): T;
  /**
   * Pipe this collection through a transform for each element of an array.
   * Returns a {@link Collection} containing one output per element.
   *
   * @param pipeFn - A function that receives this collection and one element from the array
   * @param array - An array whose elements are passed to \`pipeFn\` one by one
   * @returns A {@link Collection} with one item per element
   *
   * @example
   * // Apply each filter cutoff to the whole collection
   * $c(osc1, osc2).pipe(
   *   (col, cutoff) => $lpf(col, cutoff),
   *   ['200hz', '800hz', '3200hz'],
   * ).out()
   */
  pipe<T extends ModuleOutput | Iterable<ModuleOutput>, E>(
    pipeFn: (self: this, item: E) => T,
    array: E[]
  ): Collection;

  /**
   * Pipe this collection through a transform, then mix the original and transformed
   * signals together using a \\$mix module.
   *
   * @param pipeFn - A function that receives this collection and returns a signal to mix with the original
   * @param mix - Optional crossfade as {@link Poly<Signal>}. 0 for only original, 5 for only transformed. Default is 2.5 for equal mix.
   * @returns A Collection from the \\$mix output
   *
   * @example
   * // Mix collection with a filtered version
   * $c(osc1, osc2).pipeMix(s => $lpf(s, '1000hz')).out()
   *
   * @example
   * // Mix with different balance
   * $c(osc1, osc2).pipeMix(s => $lpf(s, '1000hz'), 1).out()
   */
  pipeMix(pipeFn: (self: this) => ModuleOutput | Collection, mix?: Poly<Signal> ): Collection;
}

/**
 * A collection of {@link ModuleOutput} instances with chainable DSP methods.
 * 
 * Created with the $() helper function. Supports iteration, indexing, and spreading.
 * Methods operate on all outputs in the collection.
 * 
 * @example
 * $(osc1, osc2, osc3).amplitude(0.5).out()  // Apply amplitude to all
 * for (const v of voices) { ... }      // Iterate
 * [...voices]                          // Spread to array
 * voices[0]                            // Index access
 * 
 * @see {@link CollectionWithRange} - for ranged outputs
 * @see {@link ModuleOutput} - individual outputs
 * @see {@link $} - helper to create Collection
 */
class Collection extends BaseCollection<ModuleOutput> {
  constructor(...outputs: ModuleOutput[]);
}

/**
 * A collection of {@link ModuleOutputWithRange} instances.
 * 
 * Created with the $r() helper function. Like {@link Collection}, but the 
 * range() method uses stored min/max values from each output.
 * 
 * @example
 * $r(lfo1, lfo2).range(0, 5).out()     // Remap using stored ranges
 * $r(...seq.gates).range(0, 1)        // Spread and remap gates
 * 
 * @see {@link Collection} - for outputs without known ranges
 * @see {@link ModuleOutputWithRange} - individual ranged outputs
 * @see {@link $r} - helper to create CollectionWithRange
 */
class CollectionWithRange extends BaseCollection<ModuleOutputWithRange> {
  constructor(...outputs: ModuleOutputWithRange[]);

  /**
   * Remap all outputs from their native ranges to a new range.
   * Uses each output's stored minValue/maxValue.
   * @param outMin - Output minimum as {@link Poly<Signal>}
   * @param outMax - Output maximum as {@link Poly<Signal>}
   * @see {@link Collection.range} - for explicit input range
   */
  override range(outMin: Poly<Signal>, outMax: Poly<Signal>): Collection;
}

/**
 * DeferredCollection is a collection of DeferredModuleOutput instances.
 * Provides a .set() method to assign signals to all contained deferred outputs.
 */
class DeferredCollection extends BaseCollection<DeferredModuleOutput> {
  constructor(...outputs: DeferredModuleOutput[]);

  /**
   * Set the signals for all deferred outputs in this collection.
   * @param polySignal - A Poly<Signal> (single signal, array, or iterable) to distribute across outputs
   */
  set(polySignal: Poly<Signal>): void;
}


// Helper functions exposed by the DSL runtime

/**
 * Convert a frequency in Hertz to a voltage value (1V/octave).
 * @param frequency - Frequency in Hz
 * @returns Voltage value for use as a {@link Signal}
 * @example $hz(440)  // A4
 * @example $hz(261.63)  // ~C4
 */
function $hz(frequency: number): number;

/**
 * Convert a note name string to a voltage value (1V/octave).
 * @param noteName - Note name like "C4", "A#3", "Bb5"
 * @returns Voltage value for use as a {@link Signal}
 * @example $note("C4")  // Middle C
 * @example $note("A4")  // 440 Hz
 */
function $note(noteName: string): number;

/**
 * Create a {@link Collection} from {@link ModuleOutput} instances.
 * 
 * Collections support chainable DSP methods, iteration, indexing, and spreading.
 * @param args - One or more {@link ModuleOutput}s to group
 * @returns A {@link Collection} of the outputs
 * @example $c(osc1, osc2).amplitude(0.5).out()
 * @example $c(osc1, osc2, osc3)[0]  // Index access
 * @example [...$c(osc1, osc2)]      // Spread to array
 * @see {@link $r} - for ranged outputs
 */
function $c(...args: (ModuleOutput | Iterable<ModuleOutput>)[]): Collection;

/**
 * Create a {@link CollectionWithRange} from {@link ModuleOutputWithRange} instances.
 * 
 * Like $() but the range() method uses stored min/max values.
 * @param args - One or more {@link ModuleOutputWithRange}s to group
 * @returns A {@link CollectionWithRange} of the outputs
 * @example $r(lfo1, lfo2).range(0, 5)  // Uses stored ranges
 * @example $r(...seq.gates).range(0, 1)
 * @see {@link $c} - for outputs without known ranges
 */
function $r(...args: (ModuleOutputWithRange | Iterable<ModuleOutputWithRange>)[]): CollectionWithRange;

/**
 * Set the global tempo for the root clock.
 * @param tempo - Tempo in BPM
 * @example $setTempo(120)  // 120 beats per minute
 * @example $setTempo(140)  // 140 beats per minute
 */
function $setTempo(tempo: number): void;

/**
 * Set the global output gain applied to the final mix.
 * @param gain - Gain as a Mono<Signal> (2.5 is default, 5.0 is unity)
 * @example $setOutputGain(2.5) // 50% gain (default)
 * @example $setOutputGain(5.0) // unity
 * @example $setOutputGain(env.out) // modulate gain from envelope
 */
function $setOutputGain(gain: Mono<Signal>): void;

/**
 * Set the time signature for the root clock.
 * Both values must be positive integers.
 * @param numerator - Beats per bar (e.g. 3, 4, 6, 7)
 * @param denominator - Beat value (e.g. 4 for quarter note, 8 for eighth note)
 * @example $setTimeSignature(4, 4)  // 4/4 time (default)
 * @example $setTimeSignature(3, 4)  // 3/4 waltz time
 * @example $setTimeSignature(6, 8)  // 6/8 compound time
 * @example $setTimeSignature(7, 8)  // 7/8 asymmetric time
 * @example $setTimeSignature(5, 4)  // 5/4 time
 */
function $setTimeSignature(numerator: number, denominator: number): void;

/**
 * Create a DeferredCollection with placeholder signals that can be assigned later.
 * Useful for feedback loops and forward references.
 * @param channels - Number of deferred outputs (1-16, default 1)
 * @example
 * const feedback = $deferred();
 * const delayed = $delay(osc.out, feedback[0]);
 * feedback.set(delayed);
 */
function $deferred(channels?: number): DeferredCollection;

/**
 * Create a slider control that binds a UI slider to a signal module.
 *
 * The slider appears in the Control panel and allows real-time parameter adjustment.
 * Dragging the slider updates both the audio engine and the source code value.
 *
 * @param label - Display label for the slider (must be a string literal)
 * @param value - Initial value (must be a numeric literal)
 * @param min - Minimum slider value
 * @param max - Maximum slider value
 * @returns A ModuleOutput carrying the slider's current value as a signal
 *
 * @example
 * const vol = $slider("Volume", 0.5, 0, 1);
 * $sine(440).amplitude(vol).out();
 */
function $slider(label: string, value: number, min: number, max: number): ModuleOutput;

/**
 * Compute the Cartesian product of the given arrays.
 *
 * Returns every possible combination of one element from each array,
 * as a typed tuple array. Pairs well with the array overload of \`.pipe()\`
 * to fan a signal across multiple parameter dimensions.
 *
 * @param arrays - Zero or more arrays to combine
 * @returns Array of typed tuples, one per combination
 *
 * @example
 * // Fan an oscillator across every combination of frequency and waveform
 * $cartesian([220, 440, 880], ['sine', 'saw']).pipe(
 *   (osc, [freq, shape]) => $oscillator({ freq, shape }).out(),
 * ).out();
 *
 * @example $cartesian([1, 2], ['a', 'b'])
 * // → [[1,'a'], [1,'b'], [2,'a'], [2,'b']]
 */
function $cartesian<A extends unknown[][]>(...arrays: A): ElementsOf<A>[];
`;

export function buildLibSource(schemas: ModuleSchema[]): string {
    // console.log('buildLibSource schemas:', schemas);
    const schemaLib = generateDSL(schemas);
    return `declare global {\n${BASE_LIB_SOURCE}\n\n${schemaLib} \n}\n\n export {};\n`;
}

type ClassSpec = {
    outputs: Array<{ name: string; description?: string }>;
    properties: Array<{
        name: string;
        schema: JSONSchema;
        description?: string;
    }>;
    rootSchema: JSONSchema;
    moduleSchema: ModuleSchema;
};

type NamespaceNode = {
    namespaces: Map<string, NamespaceNode>;
    classes: Map<string, ClassSpec>;
    order: Array<{ kind: 'namespace' | 'class'; name: string }>;
};

function makeNamespaceNode(): NamespaceNode {
    return {
        namespaces: new Map(),
        classes: new Map(),
        order: [],
    };
}

function isValidIdentifier(name: string): boolean {
    return /^[$A-Z_][0-9A-Z_$]*$/i.test(name);
}

function renderPropertyKey(name: string): string {
    return isValidIdentifier(name) ? name : JSON.stringify(name);
}

function renderReadonlyPropertyKey(name: string): string {
    return isValidIdentifier(name) ? name : `[${JSON.stringify(name)}]`;
}

function renderDocComment(description?: string, indent: string = ''): string[] {
    if (!description) return [];
    const lines = description.split(/\r?\n/);
    return [
        `${indent}/**`,
        ...lines.map((l) => `${indent} * ${l}`),
        `${indent} */`,
    ];
}

function extractParamNamesFromDoc(description?: string): string[] {
    if (!description) return [];
    const names: string[] = [];
    const re = /@param\s+([^\s]+)/g;
    for (const match of description.matchAll(re)) {
        names.push(match[1]);
    }
    return names;
}

function getMethodArgsForProperty(
    propertySchema: JSONSchema,
    rootSchema: JSONSchema,
    propertyDescription?: string,
): Array<{ name: string; type: string }> {
    const paramNames = extractParamNamesFromDoc(propertyDescription);

    // Top-level tuple expansion into multiple arguments.
    if (
        propertySchema &&
        typeof propertySchema === 'object' &&
        propertySchema.type === 'array' &&
        Array.isArray(propertySchema.prefixItems)
    ) {
        const items: JSONSchema[] = propertySchema.prefixItems;
        return items.map((itemSchema, index) => {
            const name =
                paramNames.length > 0
                    ? (paramNames[index] ?? `arg${index + 1}`)
                    : `arg${index + 1}`;
            return { name, type: schemaToTypeExpr(itemSchema, rootSchema) };
        });
    }

    // Single-argument method.
    const name = paramNames.length > 0 ? (paramNames[0] ?? 'arg1') : 'arg';
    return [{ name, type: schemaToTypeExpr(propertySchema, rootSchema) }];
}

function buildTreeFromSchemas(schemas: ModuleSchema[]): NamespaceNode {
    const root = makeNamespaceNode();

    for (const moduleSchema of schemas) {
        const fullName = String(moduleSchema.name).trim();
        if (!fullName) {
            throw new Error('ModuleSchema is missing a non-empty name');
        }

        const paramsSchema = moduleSchema.paramsSchema;
        if (!paramsSchema || typeof paramsSchema !== 'object') {
            throw new Error(`ModuleSchema ${fullName} is missing paramsSchema`);
        }

        const parts = fullName.split('.').filter((p: string) => p.length > 0);
        if (parts.length === 0) {
            throw new Error(`Invalid ModuleSchema name: ${fullName}`);
        }

        const className = parts[parts.length - 1];
        const namespacePath = parts.slice(0, -1);

        let node = root;
        for (const ns of namespacePath) {
            let child = node.namespaces.get(ns);
            if (!child) {
                child = makeNamespaceNode();
                node.namespaces.set(ns, child);
                node.order.push({ kind: 'namespace', name: ns });
            }
            node = child;
        }

        if (node.classes.has(className)) {
            throw new Error(`Duplicate class name detected: ${fullName}`);
        }
        if ('properties' in paramsSchema === false) {
            throw new Error(
                `ModuleSchema ${fullName} paramsSchema is missing properties`,
            );
        }
        const propsObj = paramsSchema.properties;
        const propsEntries =
            propsObj && typeof propsObj === 'object'
                ? Object.entries(propsObj as Record<string, JSONSchema>)
                : [];

        const properties = propsEntries.map(([name, propSchema]) => ({
            name,
            schema: propSchema,
            description: propSchema?.description,
        }));

        const outputs = (
            Array.isArray(moduleSchema.outputs) ? moduleSchema.outputs : []
        )
            .map((o) => ({
                name: String(o?.name ?? '').trim(),
                description: o?.description,
            }))
            .filter((o) => o.name.length > 0);

        node.classes.set(className, {
            outputs,
            properties,
            rootSchema: paramsSchema,
            moduleSchema,
        });
        node.order.push({ kind: 'class', name: className });
    }

    return root;
}

function renderNodeInterfaceName(baseName: string): string {
    return baseName.endsWith('Node') ? baseName : `${baseName}Node`;
}

function capitalizeName(name: string): string {
    if (!name) return name;
    return name.charAt(0).toUpperCase() + name.slice(1);
}

/**
 * Convert snake_case to camelCase
 */
function toCamelCase(str: string): string {
    return str.replace(/_([a-z])/g, (_, letter: string) =>
        letter.toUpperCase(),
    );
}

/**
 * Reserved property names that conflict with ModuleOutput, Collection, or CollectionWithRange
 * methods/properties. Output names matching these will be suffixed with an underscore.
 *
 * Single source of truth: `crates/reserved_output_names.rs`
 */
const RESERVED_OUTPUT_NAMES: ReadonlySet<string> = new Set(
    getReservedOutputNames(),
);

/**
 * Sanitize output name to avoid conflicts with reserved properties/methods.
 * Appends underscore if the camelCase name is reserved.
 */
function sanitizeOutputName(name: string): string {
    const camelName = toCamelCase(name);
    return RESERVED_OUTPUT_NAMES.has(camelName) ? `${camelName}_` : camelName;
}

/**
 * Get the output type for a single output definition
 */
function getOutputType(output: {
    polyphonic?: boolean;
    minValue?: number;
    maxValue?: number;
}): string {
    const hasRange =
        output.minValue !== undefined && output.maxValue !== undefined;
    if (output.polyphonic) {
        return hasRange ? 'CollectionWithRange' : 'Collection';
    }
    return hasRange ? 'ModuleOutputWithRange' : 'ModuleOutput';
}

/**
 * Generate interface name for multi-output modules
 */
function getMultiOutputInterfaceName(moduleSchema: ModuleSchema): string {
    const parts = moduleSchema.name
        .split('.')
        .filter((p: string) => p.length > 0);
    const baseName = parts[parts.length - 1];
    const baseNameWithoutPrefix = baseName.startsWith('$')
        ? baseName.slice(1)
        : baseName;
    return `${capitalizeName(baseNameWithoutPrefix)}Outputs`;
}

/**
 * Generate interface definition for multi-output modules.
 * The interface extends from the default output's type and includes properties for other outputs.
 */
function generateMultiOutputInterface(
    moduleSchema: ModuleSchema,
    indent: string,
): string[] {
    const outputs = moduleSchema.outputs || [];
    if (outputs.length <= 1) return [];

    // Find the default output
    const defaultOutput = outputs.find((o: any) => o.default) || outputs[0];
    const defaultOutputMeta = defaultOutput as {
        polyphonic?: boolean;
        minValue?: number;
        maxValue?: number;
    };
    const baseType = getOutputType(defaultOutputMeta);

    const interfaceName = getMultiOutputInterfaceName(moduleSchema);

    const lines: string[] = [];
    lines.push(`${indent}/**`);
    lines.push(`${indent} * Output type for ${moduleSchema.name} module.`);
    lines.push(
        `${indent} * Extends ${baseType} (default output: ${defaultOutput.name})`,
    );
    lines.push(`${indent} */`);
    lines.push(
        `${indent}export interface ${interfaceName} extends ${baseType} {`,
    );

    // Add properties for non-default outputs
    for (const output of outputs) {
        if (output.name === defaultOutput.name) continue;

        const outputMeta = output as {
            polyphonic?: boolean;
            minValue?: number;
            maxValue?: number;
            description?: string;
        };
        const outputType = getOutputType(outputMeta);
        const safeName = sanitizeOutputName(output.name);

        if (output.description) {
            lines.push(`${indent}  /** ${output.description} */`);
        }
        lines.push(`${indent}  readonly ${safeName}: ${outputType};`);
    }

    lines.push(`${indent}}`);
    return lines;
}

/**
 * Get the return type for a module factory based on its outputs
 */
function getFactoryReturnType(moduleSchema: ModuleSchema): string {
    const outputs = moduleSchema.outputs || [];

    if (outputs.length === 0) {
        return 'void';
    } else if (outputs.length === 1) {
        const output = outputs[0] as {
            polyphonic?: boolean;
            minValue?: number;
            maxValue?: number;
        };
        return getOutputType(output);
    } else {
        // Multiple outputs - return the generated interface name
        return getMultiOutputInterfaceName(moduleSchema);
    }
}

function renderFactoryFunction(
    moduleSchema: ModuleSchema,
    _interfaceName: string,
    indent: string,
): string[] {
    const functionName = moduleSchema.name.split('.').pop()!;

    let args: string[] = [];
    // @ts-ignore
    const positionalArgs = moduleSchema.positionalArgs || [];
    const schemaRequired: string[] =
        (moduleSchema.paramsSchema as any).required || [];

    // Build docstring lines
    const docLines: string[] = [];
    if (moduleSchema.documentation) {
        docLines.push(...moduleSchema.documentation.split(/\r?\n/));
    }

    const positionalRequiredness = positionalArgs.map((a: any) =>
        schemaRequired.includes(a.name),
    );

    for (let i = 0; i < positionalArgs.length; i++) {
        const arg = positionalArgs[i];
        // @ts-ignore
        const propSchema = moduleSchema.paramsSchema.properties?.[arg.name];
        // @ts-ignore
        const type = propSchema
            ? schemaToTypeExpr(propSchema, moduleSchema.paramsSchema)
            : 'any';

        const isRequired = positionalRequiredness[i];

        if (isRequired) {
            args.push(`${arg.name}: ${type}`);
        } else {
            // Check if all subsequent positional args are also optional
            const allSubsequentOptional = positionalRequiredness
                .slice(i + 1)
                .every((r: boolean) => !r);
            if (allSubsequentOptional) {
                args.push(`${arg.name}?: ${type}`);
            } else {
                args.push(`${arg.name}: ${type} | undefined`);
            }
        }

        // Add @param for positional arg
        const description = propSchema?.description;
        if (description) {
            const firstLine = description.split(/\r?\n/)[0];
            docLines.push(`@param ${arg.name} - ${firstLine}`);
        } else {
            docLines.push(`@param ${arg.name}`);
        }

        // Append enum variant descriptions as sub-bullets
        if (propSchema) {
            const variants = getEnumVariants(
                propSchema,
                moduleSchema.paramsSchema,
            );
            if (variants && variants.some((v) => v.description)) {
                for (const v of variants) {
                    const desc = v.description ? ` — ${v.description}` : '';
                    docLines.push(`  - \`${v.value}\`${desc}`);
                }
            }
        }
    }

    // @ts-ignore
    const allParamKeys = Object.keys(
        moduleSchema.paramsSchema.properties || {},
    );
    // @ts-ignore
    const positionalKeys = new Set(positionalArgs.map((a: any) => a.name));

    const configProps: string[] = [];
    const configParamDocs: string[] = [];

    for (const key of allParamKeys) {
        if (!positionalKeys.has(key)) {
            // @ts-ignore
            const propSchema = moduleSchema.paramsSchema.properties[key];
            // @ts-ignore
            const type = schemaToTypeExpr(
                propSchema,
                moduleSchema.paramsSchema,
            );
            const optionalMark = schemaRequired.includes(key) ? '' : '?';
            configProps.push(`${key}${optionalMark}: ${type}`);

            // Collect config param descriptions
            const description = propSchema?.description;
            if (description) {
                const firstLine = description.split(/\r?\n/)[0];
                configParamDocs.push(`${key} - ${firstLine}`);
            }

            // Append enum variant descriptions as sub-bullets
            const variants = getEnumVariants(
                propSchema,
                moduleSchema.paramsSchema,
            );
            if (variants && variants.some((v) => v.description)) {
                for (const v of variants) {
                    const desc = v.description ? ` — ${v.description}` : '';
                    configParamDocs.push(`    - \`${v.value}\`${desc}`);
                }
            }
        }
    }

    configProps.push(`id?: string`);

    const configType = `{ ${configProps.join('; ')} }`;

    // Config is required if any non-positional param is required
    const hasRequiredConfigProps = allParamKeys.some(
        (key: string) =>
            !positionalKeys.has(key) && schemaRequired.includes(key),
    );
    const configOptional = hasRequiredConfigProps ? '' : '?';
    args.push(`config${configOptional}: ${configType}`);

    // Add @param config with nested property descriptions
    if (configParamDocs.length > 0) {
        docLines.push(`@param config - Configuration object`);
        for (const doc of configParamDocs) {
            docLines.push(`  - ${doc}`);
        }
    } else {
        docLines.push(`@param config - Configuration object`);
    }

    // Get return type based on outputs
    const returnType = getFactoryReturnType(moduleSchema);

    const lines: string[] = [];
    if (docLines.length > 0) {
        lines.push(`${indent}/**`);
        for (const line of docLines) {
            lines.push(`${indent} * ${line}`);
        }
        lines.push(`${indent} */`);
    }
    lines.push(
        `${indent}export function ${functionName}(${args.join(', ')}): ${returnType};`,
    );

    return lines;
}

function getQualifiedNodeInterfaceType(moduleName: string): string {
    const parts = moduleName.split('.').filter((p) => p.length > 0);
    if (parts.length === 0) {
        throw new Error(`Invalid ModuleSchema name: ${moduleName}`);
    }
    const base = parts[parts.length - 1];
    const interfaceName = renderNodeInterfaceName(capitalizeName(base));
    const namespaces = parts.slice(0, -1);
    return namespaces.length > 0
        ? `${namespaces.join('.')}.${interfaceName}`
        : interfaceName;
}

function renderInterface(
    baseName: string,
    classSpec: ClassSpec,
    indent: string,
): string[] {
    const lines: string[] = [];

    // Generate multi-output interface if needed
    const multiOutputInterface = generateMultiOutputInterface(
        classSpec.moduleSchema,
        indent,
    );
    if (multiOutputInterface.length > 0) {
        lines.push(...multiOutputInterface);
        lines.push('');
    }

    // Render the factory function
    lines.push(...renderFactoryFunction(classSpec.moduleSchema, '', indent));
    return lines;
}

function renderTree(node: NamespaceNode, indentLevel: number = 0): string[] {
    const indent = '  '.repeat(indentLevel);
    const lines: string[] = [];

    for (const item of node.order) {
        if (item.kind === 'class') {
            const classSpec = node.classes.get(item.name);
            if (!classSpec) continue;
            lines.push(...renderInterface(item.name, classSpec, indent));
            lines.push('');
            continue;
        }

        const child = node.namespaces.get(item.name);
        if (!child) continue;
        lines.push(`${indent}export namespace ${item.name} {`);
        lines.push(...renderTree(child, indentLevel + 1));
        lines.push(`${indent}}`);
        lines.push('');
    }

    // Trim extra blank lines at this level.
    while (lines.length > 0 && lines[lines.length - 1] === '') {
        lines.pop();
    }
    return lines;
}

export function generateDSL(schemas: ModuleSchema[]): string {
    if (!Array.isArray(schemas)) {
        throw new Error('generateDSL expects an array of ModuleSchema');
    }
    // Filter out _clock from user-facing type declarations (it's internal only)
    const userFacingSchemas = schemas.filter((s) => s.name !== '_clock');
    const tree = buildTreeFromSchemas(userFacingSchemas);
    const lines = renderTree(tree, 0);

    // $clock is a pre-configured clock instance available to users.
    // Because the _clock factory is filtered from userFacingSchemas, its multi-output
    // interface won't have been generated by renderTree. Generate it here
    // so that `$clock` has a proper type.
    const clockSchema = schemas.find((s) => s.name === '_clock');
    if (clockSchema) {
        const clockInterface = generateMultiOutputInterface(clockSchema, '');
        if (clockInterface.length > 0) {
            lines.push('');
            lines.push(...clockInterface);
        }
        lines.push('');
        lines.push('/** Global clock module running at 120 BPM by default. */');
        const clockReturnType = getFactoryReturnType(clockSchema);
        lines.push(`export const $clock: ${clockReturnType};`);
    }

    const signalSchema = schemas.find((s) => s.name === '$signal');
    if (signalSchema) {
        lines.push('');
        lines.push('/** Input signals. */');
        const signalReturnType = getFactoryReturnType(signalSchema);
        lines.push(`export const $input: Readonly<${signalReturnType}>;`);
    }

    return lines.join('\n') + '\n';
}
