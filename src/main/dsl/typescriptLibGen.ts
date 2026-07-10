import { getReservedOutputNames } from '@modular/core';
import type { ModuleSchema } from '@modular/core';
import type {
    JSONSchema,
    Schemas,
    Schema,
} from '../../shared/dsl/schemaTypeResolver';
import {
    schemaToTypeExpr,
    getEnumVariants,
} from '../../shared/dsl/schemaTypeResolver';
import {
    dollarMethodName,
    processModuleSchema,
    qualifiesForDollarChain,
} from './paramsSchema';
import type { WavsFolderNode } from './executor';
export type { WavsFolderNode } from './executor';

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
   * [$sine('c3'), $sine('e3'), $sine('g3')].pipe(all => $mix(all)).out()
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
 * @example $sine("4s(C:major)").out()  // 4 notes of C major scale
 * @example $sine("8s(A:minor)").out()  // 8 notes of A minor scale
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
 * @example $sine("C4")           // Note string
 * @example $sine(2)              // Number
 * @example $sine("440hz")        // Hz string
 * @example $sine($signal('c').shift($sine("1hz").amp(0.1)))   // ModuleOutput (1Hz vibrato)
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
 * @example $saw(["C3", "E3", "G3"]).out()                    // 3-voice chord
 * @example $saw([...$sine("1hz"), ...$sine("2hz")]).out()   // Spread outputs into voices
 * @see {@link Signal} - for single-channel signals
 * @see {@link Collection} - for grouping outputs
 */
type Poly<T extends Signal = Signal> = OrArray<T> | Iterable<ModuleOutput>;


/**
 * A signal input that sums all channels to a single mono value.
 * Structurally identical to {@link Poly}, but signals that the module
 * combines all voices into one control signal rather than preserving polyphony.
 *
 * @example $stereoMix($saw("c3"), { width: 0.8 })          // Constant width
 * @example $stereoMix($saw("c3"), { width: $sine("1hz") }) // Oscillating stereo width
 * @see {@link Poly} - for polyphonic signals that preserve per-voice data
 * @see {@link Signal} - for single-channel signals
 */
type Mono<T extends Signal = Signal> = OrArray<T> | Iterable<ModuleOutput>;

/**
 * A phase-warp table descriptor produced by the \`$table.*\` helpers.
 *
 * Passed to modules that accept a \`Table\`-typed param (e.g. the
 * \`phase\` config field on \`$wavetable\`) to reshape a raw phase signal
 * before it is used to read a wavetable.
 *
 * Create one with \`$table.mirror\`, \`$table.bend\`, \`$table.sync\`,
 * \`$table.fold\`, or \`$table.pwm\` — do not construct directly.
 *
 * Tables are composable: \`table.pipe(next)\` feeds this table's output
 * phase into \`next\` as its input phase. Equivalent to passing \`next\`
 * as the optional second argument to any \`$table.*\` helper.
 */
type TableDescriptor =
  | { readonly type: "mirror"; readonly amount: Signal }
  | { readonly type: "bend"; readonly amount: Signal }
  | { readonly type: "sync"; readonly ratio: Signal }
  | { readonly type: "fold"; readonly amount: Signal }
  | { readonly type: "pwm"; readonly width: Signal }
  | { readonly type: "identity" }
  | { readonly type: "pipe"; readonly first: Table; readonly second: Table };

type Table = TableDescriptor & {
  /** Pass this table to \`pipeFn\` and return the result. Mirrors \`Collection.pipe\`. */
  pipe<T>(pipeFn: (self: Table) => T): T;
};

/**
 * A buffer output reference — returned by \`$buffer()\`, passed to readers
 * (like \`$bufRead\`, \`$delayRead\`) as their \`buffer\` param.
 */
type BufferOutputRef = {
  readonly type: "buffer_ref";
  readonly module: string;
  readonly port: string;
  readonly channels: number;
  readonly frameCount: number;
};

/**
 * A parsed mini-notation pattern — returned by \`$p(source)\`, passed to
 * \`$cycle\` as its pattern argument. Opaque to user code; construct with
 * \`$p(...)\` and chain \`.fast\`/\`.slow\`/\`.struct\`/\`.beat\`. Never build one
 * by hand.
 */
type ParsedPattern = {
  /**
   * Speed this pattern up by \`factor\`, mirroring Strudel's \`fast\`. The factor
   * is a constant (\`2\`) or a mini-notation number pattern (\`"2 4"\` → ×2 for
   * the first half of the cycle, ×4 for the second). A non-positive factor
   * (\`fast(0)\` or negative) is silence. Chains and nests with the other
   * pattern builders.
   */
  fast(factor: number | string): FastPattern;
  /**
   * Slow this pattern down by \`factor\` — the time-inverse of \`fast\`.
   * \`slow(n)\` equals \`fast(1 / n)\`. The factor may be a constant or a
   * mini-notation number pattern.
   */
  slow(factor: number | string): SlowPattern;
  /**
   * Impose a rhythmic structure on this pattern, mirroring Strudel's
   * \`struct\`: event timing comes from the boolean pattern, and this
   * pattern's value is sampled at each onset slot.
   *
   * | Token | Meaning |
   * |-------|---------|
   * | \`x\` (or any nonzero number) | Onset — sample this pattern here |
   * | \`~\`, \`-\`, or \`0\` | Rest (silent slot) |
   *
   * The boolean pattern supports the full mini-notation grammar —
   * \`"x ~ <x ~>"\` alternates its third slot per cycle, \`"x(3,8)"\` is a
   * euclidean structure, and so on.
   *
   * \`\`\`js
   * $cycle($p("c4 e4").struct("x ~ x x"))
   * \`\`\`
   */
  struct(boolPattern: string): StructPattern;
  /**
   * Place this pattern's value at chosen beats of a divided cycle,
   * mirroring Strudel's \`beat\`. \`t\` lists beat indices — a comma stack
   * (\`"0,7,10"\`) plays one onset per index — and \`div\` is the number of
   * beats per cycle. Each onset lasts \`1/div\` of a cycle; \`t\` wraps modulo
   * \`div\` and may be fractional. A slot not fully inside the cycle is
   * silent, as in Strudel: negative beats produce no onset, as does a
   * fractional beat within \`1\` of the cycle end. Both arguments accept a
   * constant number or a mini-notation number pattern (\`"<16 8>"\` changes
   * the grid per cycle).
   *
   * \`\`\`js
   * $cycle($p("c2").beat("0,7,10", 16))
   * \`\`\`
   */
  beat(t: number | string, div: number | string): BeatPattern;
};

/**
 * Parse a mini-notation source string into a \`ParsedPattern\` suitable
 * for \`$cycle\`. Every mini-notation literal in the DSL flows through
 * \`$p()\` — \`$cycle\` accepts a \`ParsedPattern\`, not a raw string.
 *
 * \`\`\`js
 * $cycle($p("c4 e4 g4"))                    // basic sequence
 * const bass = $p("c2 [c2 g2] c2 e2");      // reuse a parsed pattern
 * $cycle(bass)
 * \`\`\`
 *
 * The returned object is opaque (JSON-serializable; carries source +
 * span info for editor highlighting). Binding to a \`const\` preserves
 * highlighting through the indirection. Throws if \`source\` is not a
 * string or fails to parse.
 *
 * ### Atoms
 *
 * | Form | Meaning | Example |
 * |------|---------|---------|
 * | Bare number | Direct V/Oct voltage (1 V/oct CV) | \`0\`, \`1.5\`, \`-0.25\` |
 * | \`<n>hz\` | Frequency in Hz (converted to V/Oct) | \`440hz\`, \`220hz\` |
 * | Note letter (+ accidental, + octave) | Pitched note | \`c4\`, \`d#3\`, \`eb5\` |
 * | \`~\` or \`-\` | Rest (gate low) | \`'c4 ~ e4'\`, \`'c4 - e4'\` |
 *
 * ### Mini-notation
 *
 * | Syntax | Meaning | Example |
 * |--------|---------|---------|
 * | \`a b c\` | Sequence — one element per time slot | \`'c4 e4 g4'\` |
 * | \`[a b c]\` | Fast subsequence — subdivides parent slot | \`'c4 [e4 g4]'\` |
 * | \`<a b c>\` | Slow / alternating — one element per cycle | \`'<c4 e4 g4>'\` |
 * | \`a|b|c\` | Random choice each time the slot is reached | \`'c4|e4|g4'\` |
 * | \`a, b\` | Stack — comma-separated patterns play simultaneously | \`'c4 e4, g4 b4'\` |
 * | \`{a b, c d e}\` | Polymeter — children scaled to a shared step count, then stacked. \`%n\` overrides the step count: \`{a b c}%4\` | \`'{c4 e4, g4 b4 d5}'\` |
 * | \`a . b c\` | Feet — split the slot at \`.\` boundaries (useful for aligning polymeter children) | \`'{c4 . e4 g4, f4 a4 . b4}'\` |
 *
 * Grouping, stacks, polymeter, and random choice nest arbitrarily.
 *
 * ### Per-element modifiers
 *
 * Modifiers attach directly to an element (no spaces) and chain in any order.
 *
 * | Modifier | Syntax | Meaning |
 * |----------|--------|---------|
 * | Weight | \`@n\` | Relative duration within a sequence (default 1) |
 * | Elongate | \`_\` | Bare \`_\` extends the preceding step's weight by 1; \`'c4 _ _'\` is the same as \`'c4@3'\` |
 * | Speed up | \`*n\` | Repeat \`n\` times within the slot |
 * | Slow down | \`/n\` | Stretch over \`n\` cycles |
 * | Replicate | \`!n\` | Duplicate the element \`n\` times (default 2) |
 * | Degrade | \`?\` or \`?n\` | Randomly drop the element (\`?\` ≈ 50 %) |
 * | Euclidean | \`(k,n)\` or \`(k,n,offset)\` | Distribute \`k\` pulses over \`n\` steps |
 *
 * @param source - mini-notation source string
 */
declare function $p(source: string): ParsedPattern;

declare namespace $p {
/**
 * Parse a scale-degree mini-notation source and resolve each integer
 * degree to its V/Oct voltage against \`scale\`, returning a
 * \`ParsedPattern\` suitable for \`$cycle\`. Invoked as \`$p.s(source, scale)\`.
 *
 * Atoms are **0-indexed scale degrees** rather than absolute pitches:
 * \`0\` is the scale's root, \`1\` is the second scale tone, \`2\` the third,
 * and so on. Negative values move downward. Values beyond the scale
 * length wrap into higher/lower octaves automatically. Hz and note
 * atoms are rejected.
 *
 * \`\`\`js
 * $cycle($p.s("0 2 4 7", "c(major)"))       // C-major arpeggio
 * $cycle($p.s("-1 0 2 4", "a3(min)"))       // negative degrees wrap below the root
 * $cycle($p.s("0 1 2 3 4", "c(0 2 4 7 9)")) // custom intervals (pentatonic)
 * $cycle($p.s("0 4 7", "c(just)"))          // just intonation
 * \`\`\`
 *
 * ### Atoms
 *
 * | Form | Meaning | Example |
 * |------|---------|---------|
 * | Bare integer | Scale degree (0-indexed) | \`0\`, \`2\`, \`-1\` |
 * | \`~\` or \`-\` | Rest (gate low, no pitch change) | \`'0 ~ 2 ~'\`, \`'0 - 2 -'\` |
 *
 * ### Mini-notation
 *
 * | Syntax | Meaning | Example |
 * |--------|---------|---------|
 * | \`a b c\` | Sequence — one degree per time slot | \`'0 2 4'\` |
 * | \`[a b c]\` | Fast subsequence — subdivides parent slot | \`'0 [2 4]'\` |
 * | \`<a b c>\` | Slow / alternating — one element per cycle | \`'<0 4 7>'\` |
 * | \`a|b|c\` | Random choice each time the slot is reached | \`'0|2|4'\` |
 * | \`a, b\` | Stack — comma-separated patterns play simultaneously | \`'0 2, 4 7'\` |
 * | \`{a b, c d e}\` | Polymeter — children scaled to a shared step count, then stacked. \`%n\` overrides the step count: \`{a b c}%4\` | \`'{0 2, 4 5 7}'\` |
 * | \`a . b c\` | Feet — split the slot at \`.\` boundaries (useful for aligning polymeter children) | \`'{0 . 2 4, 5 7 . 9}'\` |
 *
 * Grouping, stacks, polymeter, and random choice nest arbitrarily.
 * Modifiers attach directly to an element (no spaces) and chain in any
 * order:
 *
 * | Modifier | Syntax | Meaning |
 * |----------|--------|---------|
 * | Weight | \`@n\` | Relative duration within a sequence (default 1) |
 * | Elongate | \`_\` | Bare \`_\` extends the preceding step's weight by 1; \`'0 _ _'\` is the same as \`'0@3'\` |
 * | Speed up | \`*n\` | Repeat \`n\` times within the slot |
 * | Slow down | \`/n\` | Stretch over \`n\` cycles |
 * | Replicate | \`!n\` | Duplicate the element \`n\` times (default 2) |
 * | Degrade | \`?\` or \`?n\` | Randomly drop the element (\`?\` ≈ 50 %) |
 * | Euclidean | \`(k,n)\` or \`(k,n,offset)\` | Distribute \`k\` pulses over \`n\` steps |
 *
 * ### Scale strings
 *
 * | Form | Example | Meaning |
 * |------|---------|---------|
 * | \`tonic(name)\` | \`"c(major)"\`, \`"d#(min)"\`, \`"f(dorian)"\` | Named scale type rooted at the tonic |
 * | \`tonic<octave>(name)\` | \`"c3(major)"\`, \`"D#4(min)"\` | Same with an explicit octave (default 4 → root = C4 = 0 V) |
 * | \`tonic(custom intervals)\` | \`"c(0 2 4 5 7 9 11)"\` | Custom semitone offsets from the tonic |
 * | \`tonic(just)\` / \`tonic(pythagorean)\` | \`"c(just)"\`, \`"a(pythag)"\` | Non-equal 12-tone tunings |
 * | \`chromatic\` | \`"chromatic"\` | All 12 semitones, 12-TET |
 *
 * Recognized scale names include major, minor, ionian, dorian, phrygian,
 * lydian, mixolydian, aeolian, locrian, harmonic/melodic minor,
 * pentatonic major/minor, blues, whole tone.
 *
 * ### Chaining
 *
 * The returned \`SpPattern\` is chainable with \`.add(...)\` and \`.sub(...)\`.
 * Each accepts another scale-degree mini-notation string and combines
 * the two patterns. Bare \`.add(x)\` defaults to \`.add.in(x)\`; explicit
 * mode methods cover all seven Strudel alignments:
 *
 * | Mode | Behaviour |
 * |------|-----------|
 * | \`.in\` (default) | Left pattern's onsets drive timing; right is sampled at each left event. |
 * | \`.out\` | Right pattern's onsets drive timing; left is sampled at each right event. |
 * | \`.mix\` | Output events at every intersection of left + right onsets. |
 * | \`.squeeze\` | Each left event nests a full cycle of the right pattern. |
 * | \`.squeezeout\` | Each right event nests a full cycle of the left pattern. |
 * | \`.reset\` | Right pattern retriggers the left, aligned to cycle position. |
 * | \`.restart\` | Right pattern retriggers the left, aligned to cycle 0. |
 *
 * \`\`\`js
 * $cycle($p.s("0 1 2", "c(maj)").add("0 2"))               // .add defaults to .add.in
 * $cycle($p.s("0 1 2", "d#(min)").add.in("10 20"))
 * $cycle($p.s("0 1 2", "c(maj)").sub.squeeze("1 2 3"))
 * \`\`\`
 *
 * @param source - integer scale-degree mini-notation source
 * @param scale - scale string, e.g. "c(major)", "D#3(min)", "a(just)"
 */
function s(source: string, scale: string): SpPattern;

/**
 * Arrange patterns over multiple cycles, mirroring Strudel's \`arrange\`.
 * Each argument is a \`[cycles, pattern]\` tuple: the \`pattern\` (a \`$p(...)\`,
 * \`$p.s(...)\`, or nested \`$p.arrange(...)\`) plays for \`cycles\` cycles, and
 * the sections play back-to-back, looping with period \`Σ cycles\`.
 *
 * Because each \`$p.s\` section resolves through its own scale, an arrangement
 * can switch scales (or keys) between sections.
 *
 * \`\`\`js
 * // 4 cycles of a C-major arp, then 2 of an A-minor arp, looping every 6
 * $cycle($p.arrange(
 *   [4, $p.s("0 2 4 7", "c(major)")],
 *   [2, $p.s("0 2 4",   "a(min)")],
 * ))
 * \`\`\`
 *
 * Cycle counts must be **positive integers**, except a single **trailing**
 * section may use \`Infinity\` to loop forever once reached (any section after
 * an \`Infinity\` section could never play and is rejected):
 *
 * \`\`\`js
 * $cycle($p.arrange(
 *   [2, $p("c4 e4 g4")],            // intro, played once
 *   [Infinity, $p.s("0 2 4", "f(lydian)")],  // then loops forever
 * ))
 * \`\`\`
 *
 * The result is an \`ArrangePattern\`, itself usable as a section of another
 * \`$p.arrange(...)\` (arrangements nest) or as a \`$cycle\` argument.
 *
 * @param sections - \`[cycles, pattern]\` tuples, in play order
 */
function arrange(
  ...sections: [number, ParsedPattern | SpPattern | ArrangePattern | FastPattern | SlowPattern | StructPattern | BeatPattern][]
): ArrangePattern;
}

/**
 * One Strudel-style alignment for a \`$p.s\` chain op.
 */
type SpAlignmentMode =
  | 'in'
  | 'out'
  | 'mix'
  | 'squeeze'
  | 'squeezeout'
  | 'reset'
  | 'restart';

/**
 * Callable + method-bag returned by \`.add\` / \`.sub\` on an \`SpPattern\`.
 * Bare invocation aliases the \`.in\` method.
 */
type SpCombineBuilder = ((rhs: string) => SpPattern) & {
  readonly [M in SpAlignmentMode]: (rhs: string) => SpPattern;
};

/**
 * Chainable scale-degree pattern returned by \`$p.s()\`. Pass directly to
 * \`$cycle\`'s \`pattern\` param, or chain \`.add\`/\`.sub\`/\`.fast\`/\`.slow\`/
 * \`.struct\`/\`.beat\`. Opaque to user code — chain methods return fresh
 * patterns.
 */
type SpPattern = {
  add: SpCombineBuilder;
  sub: SpCombineBuilder;
  /** Speed this pattern up by \`factor\`. See \`$p(...).fast\`. */
  fast(factor: number | string): FastPattern;
  /** Slow this pattern down by \`factor\`. See \`$p(...).slow\`. */
  slow(factor: number | string): SlowPattern;
  /** Impose a boolean rhythmic structure. See \`$p(...).struct\`. */
  struct(boolPattern: string): StructPattern;
  /** Place this pattern at beats \`t\` of a \`div\`-beat cycle. See \`$p(...).beat\`. */
  beat(t: number | string, div: number | string): BeatPattern;
};

/**
 * An arrangement returned by \`$p.arrange(...)\`. Pass directly to \`$cycle\`'s
 * \`pattern\` param, or nest it as a section of another \`$p.arrange(...)\`.
 * Opaque to user code — construct with \`$p.arrange(...)\`; never build one by
 * hand. A \`cycles\` of \`'Infinity'\` (only the last section) loops forever.
 */
type ArrangePattern = {
  /** Speed this arrangement up by \`factor\`. See \`$p(...).fast\`. */
  fast(factor: number | string): FastPattern;
  /** Slow this arrangement down by \`factor\`. See \`$p(...).slow\`. */
  slow(factor: number | string): SlowPattern;
  /** Impose a boolean rhythmic structure. See \`$p(...).struct\`. */
  struct(boolPattern: string): StructPattern;
  /** Place this arrangement at beats \`t\` of a \`div\`-beat cycle. See \`$p(...).beat\`. */
  beat(t: number | string, div: number | string): BeatPattern;
};

/**
 * A sped-up pattern returned by \`pattern.fast(factor)\` (Strudel's \`fast\`).
 * Pass directly to \`$cycle\`'s \`pattern\` param, nest it as an \`$p.arrange(...)\`
 * section, or chain further pattern methods. Opaque to user code — construct
 * with \`.fast(...)\`; never build one by hand.
 */
type FastPattern = {
  /** Speed this pattern up further by \`factor\`. See \`$p(...).fast\`. */
  fast(factor: number | string): FastPattern;
  /** Slow this pattern down by \`factor\`. See \`$p(...).slow\`. */
  slow(factor: number | string): SlowPattern;
  /** Impose a boolean rhythmic structure. See \`$p(...).struct\`. */
  struct(boolPattern: string): StructPattern;
  /** Place this pattern at beats \`t\` of a \`div\`-beat cycle. See \`$p(...).beat\`. */
  beat(t: number | string, div: number | string): BeatPattern;
};

/**
 * A slowed-down pattern returned by \`pattern.slow(factor)\` (Strudel's \`slow\`).
 * The time-inverse of \`FastPattern\`; same composition rules.
 */
type SlowPattern = {
  /** Speed this pattern up by \`factor\`. See \`$p(...).fast\`. */
  fast(factor: number | string): FastPattern;
  /** Slow this pattern down further by \`factor\`. See \`$p(...).slow\`. */
  slow(factor: number | string): SlowPattern;
  /** Impose a boolean rhythmic structure. See \`$p(...).struct\`. */
  struct(boolPattern: string): StructPattern;
  /** Place this pattern at beats \`t\` of a \`div\`-beat cycle. See \`$p(...).beat\`. */
  beat(t: number | string, div: number | string): BeatPattern;
};

/**
 * A structured pattern returned by \`pattern.struct(boolPattern)\` (Strudel's
 * \`struct\`): timing from the boolean pattern, values sampled from the source.
 * Pass directly to \`$cycle\`'s \`pattern\` param, nest it as an
 * \`$p.arrange(...)\` section, or chain further pattern methods. Opaque to
 * user code — construct with \`.struct(...)\`; never build one by hand.
 *
 * \`\`\`js
 * $cycle($p("c4 e4").struct("x ~ <x ~>"))
 * \`\`\`
 */
type StructPattern = {
  /** Speed this pattern up by \`factor\`. See \`$p(...).fast\`. */
  fast(factor: number | string): FastPattern;
  /** Slow this pattern down by \`factor\`. See \`$p(...).slow\`. */
  slow(factor: number | string): SlowPattern;
  /** Impose another boolean rhythmic structure. See \`$p(...).struct\`. */
  struct(boolPattern: string): StructPattern;
  /** Place this pattern at beats \`t\` of a \`div\`-beat cycle. See \`$p(...).beat\`. */
  beat(t: number | string, div: number | string): BeatPattern;
};

/**
 * A beat-grid pattern returned by \`pattern.beat(t, div)\` (Strudel's \`beat\`):
 * the source value plays at beat \`t\` of a \`div\`-beat cycle, silence
 * elsewhere. Pass directly to \`$cycle\`'s \`pattern\` param, nest it as an
 * \`$p.arrange(...)\` section, or chain further pattern methods. Opaque to
 * user code — construct with \`.beat(...)\`; never build one by hand.
 *
 * \`\`\`js
 * $cycle($p("c2 g2").beat("0,7,10", 16))
 * \`\`\`
 */
type BeatPattern = {
  /** Speed this pattern up by \`factor\`. See \`$p(...).fast\`. */
  fast(factor: number | string): FastPattern;
  /** Slow this pattern down by \`factor\`. See \`$p(...).slow\`. */
  slow(factor: number | string): SlowPattern;
  /** Impose a boolean rhythmic structure. See \`$p(...).struct\`. */
  struct(boolPattern: string): StructPattern;
  /** Place this pattern at other beats. See \`$p(...).beat\`. */
  beat(t: number | string, div: number | string): BeatPattern;
};

/**
 * A loaded WAV sample handle — returned by \`$wavs()\`, passed to \`$sampler()\` as the \`wav\` param.
 */
type WavHandle = {
  readonly type: 'wav_ref';
  readonly path: string;
  readonly channels: number;
  readonly sampleRate: number;
  readonly frameCount: number;
  readonly duration: number;
  readonly bitDepth: number;
  /** File modification time (epoch ms). Cache-key hint — changes when the WAV is edited on disk. */
  readonly mtime: number;
  readonly pitch?: number;
  readonly playback?: 'one-shot' | 'loop';
  readonly bpm?: number;
  readonly beats?: number;
  readonly timeSignature?: {
    readonly num: number;
    readonly den: number;
  };
  /** Number of bars the sample spans, computed from BPM and time signature. E.g. an exact 2-bar loop is \`2.0\`; a 2.64-bar buffer is \`2.64\`. Absent when no BPM could be derived. */
  readonly barCount?: number;
  readonly loops: ReadonlyArray<{
    readonly type: 'forward' | 'pingpong' | 'backward';
    readonly start: number;
    readonly end: number;
  }>;
  readonly cuePoints: ReadonlyArray<{
    readonly position: number;
    readonly label: string;
  }>;
};

/**
 * Options for stereo output routing via the out() method.
 * @see {@link ModuleOutput.out}
 * @see {@link Collection.out}
 */
interface StereoOutOptions {
  /** Base output channel (0-15, default 0). Left plays on baseChannel, right on baseChannel+1 */
  baseChannel?: number;
  /** Output gain. If set, a $scaleAndShift module is added after the stereo mix */
  gain?: Poly<Signal>;
  /** Pan position (-5 = left, 0 = center, +5 = right). Default 0 */
  pan?: Poly<Signal>;
  /** Stereo width/spread (0 = no spread, 5 = full spread). Default 0 */
  width?: Mono<Signal>;
  /** Label shown on this output's VU meter. Must be unique across outs */
  label?: string;
  /** Silence this output. It is still metered; its VU meter greys out */
  mute?: boolean;
  /** When any output is soloed, only soloed outputs are audible */
  solo?: boolean;
}

/**
 * Options for mono output routing via the outMono() method.
 * @see {@link ModuleOutput.outMono}
 * @see {@link Collection.outMono}
 */
interface MonoOutOptions {
  /** Output channel (0-15, default 0). Wins over the positional channel argument */
  channel?: number;
  /** Output gain. If set, a $scaleAndShift module is added after the mix */
  gain?: Poly<Signal>;
  /** Label shown on this output's VU meter. Must be unique across outs */
  label?: string;
  /** Silence this output. It is still metered; its VU meter greys out */
  mute?: boolean;
  /** When any output is soloed, only soloed outputs are audible */
  solo?: boolean;
}

/**
 * A single output from a module, representing a mono signal connection.
 * 
 * ModuleOutputs are chainable - methods like amplitude(), shift(), and out() 
 * return the same output for fluent API usage.
 * 
 * @example
 * const osc = $sine("C4")
 * osc.amplitude(0.5).out()      // Chain methods
 * osc.scope().out()             // Add visualization
 * $lpf(osc, "800hz").out()      // Use as input
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
     * Scale the signal by a linear factor (5 = unity, 2.5 = half, 10 = 2x).
     * Creates a $scaleAndShift module internally.
     *
     * For perceptual (audio-taper) volume control, use {@link gain} instead.
     * @param factor - Scale factor as {@link Poly<Signal>}
     * @returns The scaled {@link Collection} for chaining
     * @example $sine("c4").amplitude(2.5)  // Half amplitude
     */
   amplitude(factor: Poly<Signal>): Collection;

   /** Alias for {@link amplitude} */
   amp(factor: Poly<Signal>): Collection;
  
  /**
   * Add a DC offset to the signal. Creates a $scaleAndShift module internally.
   * @param offset - Offset value as {@link Poly<Signal>}
   * @returns The shifted {@link Collection} for chaining
   * @example $sine("1hz").shift(2.5)  // Add a +2.5V DC offset
   */
  shift(offset: Poly<Signal>): Collection;

  /**
   * Offset this pitch by an absolute frequency amount, in Hz. The V/Oct signal
   * is converted to Hz, the offset added, then converted back. Creates an
   * $addHz module internally.
   * @param offset - Hz offset as {@link Poly<Signal>}
   * @returns The retuned {@link Collection} for chaining
   * @example $saw('C4').addHz(0.5)  // slight detune
   */
  addHz(offset: Poly<Signal>): Collection;

  /**
   * Multiply this pitch by a frequency factor (2 = octave up, 0.5 = down).
   * The V/Oct signal is converted to Hz, multiplied, then converted back.
   * Creates a $mulHz module internally.
   * @param factor - Frequency multiplier as {@link Poly<Signal>}
   * @returns The retuned {@link Collection} for chaining
   * @example $saw('C4').mulHz(1.5)  // up a just fifth
   */
  mulHz(factor: Poly<Signal>): Collection;

    /**
     * Scale the signal by a factor with a perceptual (audio taper) curve
     * (5 = unity, 0 = silence).
     *
     * For linear amplitude scaling, use {@link amplitude} instead.
     * @param level - Amplitude level as {@link Poly<Signal>}
     * @returns The scaled {@link Collection} for chaining
     * @example $sine("c4").gain(2.5)
     */
   gain(level: Poly<Signal>): Collection;

  /**
   * Apply a power curve to this signal. Creates a \\$curve module internally.
   * @param factor - Exponent for the curve (default 3)
   * @returns The curved {@link Collection} for chaining
   * @example $sine("1hz").exp(2)  // Quadratic curve
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
   */
  scope(config?: { msPerFrame?: number; triggerThreshold?: number; triggerWaitToRender?: boolean; range?: [number, number] }): this;
  
  /**
   * Send this output to speakers as stereo.
   * @param options - Stereo output options ({@link StereoOutOptions})
   * @example $sine("c4").out({ gain: 2.5, pan: -2 })
   * @example $sine("c4").out({ label: 'lead' })
   */
  out(options?: StereoOutOptions): this;

  /**
   * Send this output to speakers as mono.
   * @param channelOrOptions - Output channel (0-15, default 0), or mono output options ({@link MonoOutOptions})
    * @param gainOrOptions - Output gain as {@link Poly<Signal>}, or the same options (an explicit channel there wins)
    * @example $sine("1hz").outMono(2, 0.3)
    * @example $sine("c3").outMono({ channel: 2, gain: 2.5, label: 'sub' })
    */
   outMono(channelOrOptions?: number | MonoOutOptions, gainOrOptions?: Poly<Signal> | MonoOutOptions): this;

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
   *   (s, cut) => $lpf(s, cut),
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
   * Fold this output's channels down to a target channel count by panning them
   * evenly across the output field with an equal-power law. Creates a \\$mixDown
   * module internally.
   *
   * @param channels - Target output channel count (1–16). Defaults to 1 (mono).
   * @param mode - How channels landing on the same output combine. Defaults to "sum".
   * @returns A Collection from the \\$mixDown output
   *
   * @example
   * // Fold a 3-voice spread down to stereo
   * $saw($spread(0, 5, 3)).mix(2).out()
   */
  mix(channels?: number, mode?: "sum" | "average" | "max" | "min"): Collection;

  /**
   * Remap this output from an explicit input range to a new output range.
   * Creates a $remap module internally.
   * @param outMin - New minimum as {@link Poly<Signal>}
   * @param outMax - New maximum as {@link Poly<Signal>}
   * @param inMin - Input minimum as {@link Poly<Signal>}
   * @param inMax - Input maximum as {@link Poly<Signal>}
   * @returns A {@link CollectionWithRange} carrying the remapped signal
   * @example $sine('c4').range(0, 1, -5, 5)
   */
  range(outMin: Poly<Signal>, outMax: Poly<Signal>, inMin: Poly<Signal>, inMax: Poly<Signal>): CollectionWithRange;

  /**
   * Register this output as a send to a bus, with optional gain.
   * @param bus - The {@link Bus} to send to
   * @param gain - Send level as {@link Poly<Signal>}
   * @returns This output for chaining
   */
  send(bus: Bus, gain?: Poly<Signal>): this;

  /**
   * Chainable module namespace. Every module whose first argument is a
   * {@link Poly<Signal>} becomes a method here, receiving this output as that
   * argument.
   * @example $sine(0).$.lpf('100hz')  // ≡ $lpf($sine(0), '100hz')
   */
  readonly $: DollarChain;

  /**
   * Like {@link $}, but each method takes a leading \`mix\` signal that
   * crossfades the dry input against the wet result (0 = dry, 5 = wet,
   * 2.5 = equal).
   * @example $sine(0).$m.lpf(2.5, '100hz')
   */
  readonly $m: DollarMixChain;
}

/**
 * DeferredModuleOutput is a placeholder for a signal that will be assigned later.
 * Useful for feedback loops and forward references in the DSL.
 * Supports the same chainable methods as ModuleOutput (amplitude, shift, scope, out, outMono).
 */
interface DeferredModuleOutput extends ModuleOutput {
  /**
   * Set the actual signal this deferred output should resolve to.
   * Bare {@link Signal} literals (numbers, note/Hz strings) are lifted into
   * $signal modules, matching $c.
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
 * const lfo = $sine('2hz')             // Outputs -5 to +5
 * lfo.range(200, 2000)                 // Remap to 200-2000
 * $adsr($clock.beatTrigger, { attack: 0.1 }).range(0, 1)
 *
 * @see {@link ModuleOutput} - base interface
 * @see {@link CollectionWithRange} - for collections of ranged outputs
 */
interface ModuleOutputWithRange extends ModuleOutput {
  /** The minimum value this output produces */
  readonly minValue: Signal;
  /** The maximum value this output produces */
  readonly maxValue: Signal;
  
  /**
   * Remap the output from its native range to a new range.
   * Uses the stored minValue/maxValue automatically.
   * @param outMin - New minimum as {@link Poly<Signal>}
   * @param outMax - New maximum as {@link Poly<Signal>}
   * @returns A {@link CollectionWithRange} carrying the remapped signal
   * @example $sine("1hz").range("C3", "C5")
   */
  range(outMin: Poly<Signal>, outMax: Poly<Signal>): CollectionWithRange;
}


class BaseCollection<T extends ModuleOutput> implements Iterable<T> {
  /** Number of outputs in the collection */
  readonly length: number;
  /** Index access to individual elements */
  readonly [index: number]: T;
  [Symbol.iterator](): Iterator<T>;

    /**
     * Scale all signals by a linear factor (5 = unity, 2.5 = half, 10 = 2x).
     *
     * For perceptual (audio-taper) volume control, use {@link gain} instead.
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
   * Offset all pitches by an absolute frequency amount, in Hz.
   * @param offset - Hz offset as {@link Poly<Signal>}
   * @see {@link ModuleOutput.addHz}
   */
  addHz(offset: Poly<Signal>): Collection;

  /**
   * Multiply all pitches by a frequency factor (2 = octave up, 0.5 = down).
   * @param factor - Frequency multiplier as {@link Poly<Signal>}
   * @see {@link ModuleOutput.mulHz}
   */
  mulHz(factor: Poly<Signal>): Collection;

    /**
     * Scale all signals by a factor with a perceptual (audio taper) curve
     * (5 = unity, 0 = silence).
     *
     * For linear amplitude scaling, use {@link amplitude} instead.
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
   * @param options - Stereo output options ({@link StereoOutOptions})
   * @example $saw(['c3', 'e3', 'g3']).out({ label: 'chord' })
   */
  out(options?: StereoOutOptions): this;

  /**
   * Send all outputs to speakers as mono, summed together.
   * @param channelOrOptions - Output channel (0-15, default 0), or mono output options ({@link MonoOutOptions})
    * @param gainOrOptions - Output gain as {@link Poly<Signal>}, or the same options (an explicit channel there wins)
    * @example $saw(['c2', 'c3']).outMono(0, { label: 'bass' })
    */
   outMono(channelOrOptions?: number | MonoOutOptions, gainOrOptions?: Poly<Signal> | MonoOutOptions): this;


  /**
   * Remap all outputs from input range to output range.
   * Requires explicit input min/max values.
   * @param inMin - Input minimum as {@link Poly<Signal>}
   * @param inMax - Input maximum as {@link Poly<Signal>}
   * @param outMin - Output minimum as {@link Poly<Signal>}
   * @param outMax - Output maximum as {@link Poly<Signal>}
   * @see {@link CollectionWithRange.range} - for automatic input range
   */
  range(outMin: Poly<Signal>, outMax: Poly<Signal>, inMin: Poly<Signal>, inMax: Poly<Signal>): CollectionWithRange;

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
   * $c($sine('c3'), $sine('e3')).pipe(all => all.amplitude(2.5)).out()
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
   * $c($sine('c3'), $sine('e3')).pipe(
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
   * $c($sine('c3'), $sine('e3')).pipeMix(s => $lpf(s, '1000hz')).out()
   *
   * @example
   * // Mix with different balance
   * $c($sine('c3'), $sine('e3')).pipeMix(s => $lpf(s, '1000hz'), 1).out()
   */
  pipeMix(pipeFn: (self: this) => ModuleOutput | Collection, mix?: Poly<Signal> ): Collection;

  /**
   * Fold this collection's channels down to a target channel count by panning
   * them evenly across the output field with an equal-power law. Creates a
   * \\$mixDown module internally.
   *
   * @param channels - Target output channel count (1–16). Defaults to 1 (mono).
   * @param mode - How channels landing on the same output combine. Defaults to "sum".
   * @returns A Collection from the \\$mixDown output
   *
   * @example
   * // Fold a 3-voice spread down to stereo
   * $saw($spread(0, 5, 3)).mix(2).out()
   */
  mix(channels?: number, mode?: "sum" | "average" | "max" | "min"): Collection;

  /**
   * Register all outputs in this collection as a send to a bus, with optional gain.
   * @param bus - The {@link Bus} to send to
   * @param gain - Send level as {@link Poly<Signal>}
   * @returns This collection for chaining
   */
  send(bus: Bus, gain?: Poly<Signal>): this;

  /**
   * Chainable module namespace. Every module whose first argument is a
   * {@link Poly<Signal>} becomes a method here, receiving this collection as
   * that argument.
   * @example $c($sine('c3'), $sine('e3')).$.lpf('100hz')  // ≡ $lpf($c($sine('c3'), $sine('e3')), '100hz')
   */
  readonly $: DollarChain;

  /**
   * Like {@link $}, but each method takes a leading \`mix\` signal that
   * crossfades the dry input against the wet result (0 = dry, 5 = wet,
   * 2.5 = equal).
   * @example $c($sine('c3'), $sine('e3')).$m.lpf(2.5, '100hz')
   */
  readonly $m: DollarMixChain;
}

/**
 * A collection of {@link ModuleOutput} instances with chainable DSP methods.
 * 
 * Created with the $c() helper function. Supports iteration, indexing, and spreading.
 * Methods operate on all outputs in the collection.
 * 
 * @example
 * const voices = $c($sine('c3'), $sine('e3'), $sine('g3'))
 * voices.amplitude(0.5).out()          // Apply amplitude to all
 * for (const v of voices) v.scope()    // Iterate
 * const arr = [...voices]              // Spread to array
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
 * $r($sine('1hz'), $sine('2hz')).range(0, 5).out()  // Remap using stored ranges
 * $r(...[$sine('3hz'), $sine('4hz')]).range(0, 1)   // Spread and remap
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
  override range(outMin: Poly<Signal>, outMax: Poly<Signal>): CollectionWithRange;
}

/**
 * DeferredCollection is a collection of DeferredModuleOutput instances.
 * Provides a .set() method to assign signals to all contained deferred outputs.
 */
class DeferredCollection extends BaseCollection<DeferredModuleOutput> {
  constructor(...outputs: DeferredModuleOutput[]);

  /**
   * Set the signals for all deferred outputs in this collection, cycling a
   * narrower argument across the channels. Bare {@link Signal} literals
   * (numbers, note/Hz strings) are lifted into $signal modules, matching $c —
   * a string is one signal, not spread into characters.
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
 * Bare {@link Signal} literals (numbers, note/Hz strings) are lifted into
 * $signal modules, so they can be mixed in alongside module outputs.
 * @param args - One or more {@link ModuleOutput}s or {@link Signal} literals to group
 * @returns A {@link Collection} of the outputs
 * @example $c($sine('c3'), $sine('e3')).amplitude(0.5).out()
 * @example $c(440, 'c4', $sine('e3'))  // Bare number/string lifted into $signal
 * @example $c($sine('c3'), $sine('e3'), $sine('g3'))[0]  // Index access
 * @example [...$c($sine('c3'), $sine('e3'))]             // Spread to array
 * @see {@link $r} - for ranged outputs
 */
function $c(...args: (Signal | Iterable<Signal>)[]): Collection;

/**
 * Create a {@link CollectionWithRange} from {@link ModuleOutputWithRange} instances.
 * 
 * Like $() but the range() method uses stored min/max values.
 * @param args - One or more {@link ModuleOutputWithRange}s to group
 * @returns A {@link CollectionWithRange} of the outputs
 * @example $r($sine('1hz'), $sine('2hz')).range(0, 5)  // Uses stored ranges
 * @example $r(...[$sine('3hz'), $sine('4hz')]).range(0, 1)
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
 * @example $setOutputGain($adsr($clock.beatTrigger, {})) // modulate gain from envelope
 */
function $setOutputGain(gain: Mono<Signal>): void;

/**
 * Render a Lissajous-style XY oscilloscope as the editor background.
 * Each axis is cycled to the longer arity, producing one trace per pair.
 * Bare {@link Signal} literals (numbers, note/Hz strings) are lifted into
 * $signal modules, matching $c.
 * Last call wins; only one global XY scope can be active at a time.
 * @param x - Horizontal channel(s)
 * @param y - Vertical channel(s)
 * @param config.xRange - Horizontal voltage window (default [-5, 5])
 * @param config.yRange - Vertical voltage window (default [-5, 5])
 * @example $scopeXY($sine($hz(440)), $sine($hz(311)))
 * @example $scopeXY($c($sine('c3'), $sine('e3')), $sine('g3')) // 2 traces, both share the same Y
 */
function $scopeXY(
  x: Poly<Signal>,
  y: Poly<Signal>,
  config?: { xRange?: [number, number]; yRange?: [number, number] },
): void;

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
 * @param channels - Number of deferred outputs (1-64, default 1)
 * @example
 * const fb = $deferred();
 * // the delay's buffer breaks the cycle, so the deferred can feed back in
 * const echoed = $delay($noise('white').amp(fb[0]), 0.25);
 * fb.set(echoed);
 * echoed.out();
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
 * @returns A CollectionWithRange carrying the slider's current value (range [min, max])
 *
 * @example
 * const vol = $slider("Volume", 0.5, 0, 1);
 * $sine(440).amplitude(vol).out();
 */
function $slider(label: string, value: number, min: number, max: number): CollectionWithRange;

/**
 * A send-return bus. Create one with {@link $bus}, then call \`.send(bus, gain)\` on
 * any {@link ModuleOutput} or {@link Collection} to route signals through it.
 * The bus callback receives a mixed {@link Collection} of all sends.
 */
class Bus {
  /** @internal */
  private constructor();
}

/**
 * Create a send-return bus.
 *
 * The callback receives a {@link Collection} that is the mix of all signals
 * sent to this bus via \`.send(bus, gain)\`. Use it to add effects or route the
 * mixed signal to an output.
 *
 * @param cb - Called during patch finalization with the mixed sends.
 *             The return value of this function is discarded, it's up to the cb to
 *             call \`.out()\` or \`.outMono()\` to actually hear anything.
 * @returns A {@link Bus} handle passed to \`.send()\`
 *
 * @example
 * const reverb = $bus((mixed) => $plate(mixed).out());
 * $saw('a').send(reverb, 0.6);
 * $sine('a2').send(reverb, 0.4);
 */
function $bus(cb: (mixed: Collection) => unknown): Bus;

/**
 * Set a custom end-of-chain processor applied to the final mix before output gain.
 *
 * The callback receives the fully mixed {@link Collection} and should return a
 * processed signal. It is called once during patch finalization.
 *
 * @param cb - Transform applied to the final mix
 *
 * @example
 * $setEndOfChainCb((mix) => $lpf(mix, '2000hz'));
 */
function $setEndOfChainCb(cb: (mixed: Collection) => ModuleOutput | Collection | CollectionWithRange): void;

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
 * // Fan oscillators across every combination of frequency and waveform
 * $mix(
 *   $cartesian([220, 440], ['sine', 'saw']).map(([freq, shape]) =>
 *     shape === 'sine' ? $sine($hz(freq)) : $saw($hz(freq)),
 *   ),
 * ).out();
 *
 * @example $cartesian([1, 2], ['a', 'b'])
 * // → [[1,'a'], [1,'b'], [2,'a'], [2,'b']]
 */
function $cartesian<A extends unknown[][]>(...arrays: A): ElementsOf<A>[];

/**
 * \`$ott\` — three-band upward + downward compressor in the style of Xfer's OTT.
 *
 * Splits the input into low / mid / high via \`$xover\`, then runs each band
 * through \`$comp\` with both upward and downward compression engaged at fast
 * attack/release ballistics. Bands are summed and crossfaded against the
 * original input via \`depth\`.
 *
 * Per-band trim (\`lowGain\` / \`midGain\` / \`highGain\`) follows the
 * \`$scaleAndShift\` convention: 5 V = unity, 0 V = silence, 10 V = +6 dB.
 *
 * \`\`\`js
 * $ott($saw('c2')).out()
 * $ott($mix([$saw('c2'), $noise('white')]), { depth: 4, lowGain: 6, highGain: 4, threshold: 1.5 }).out()
 * \`\`\`
 */
function $ott(input: Collection | ModuleOutput, config?: {
    /**
     * Optional side-chain detector signal. The same crossover network splits
     * the sidechain into low/mid/high and each band's compressor keys off the
     * matching band — the gain is still applied to \`input\`.
     */
    sidechain?: Collection | ModuleOutput;
    /** wet/dry blend, 0–5 (default 5 = fully wet) */
    depth?: Poly<Signal>;
    /** low/mid crossover (V/Oct, default ~120 Hz) */
    lowMidFreq?: Poly<Signal>;
    /** mid/high crossover (V/Oct, default ~2500 Hz) */
    midHighFreq?: Poly<Signal>;
    /** downward stage threshold in volts (default 1.0) */
    threshold?: Poly<Signal>;
    /** downward stage ratio: > 1 compresses, < 1 expands (boosts loud), 1 = passthrough. Default 4 */
    ratio?: Poly<Signal>;
    /** upward stage threshold in volts (default 0.5) */
    upwardThreshold?: Poly<Signal>;
    /** upward stage ratio: > 1 boosts quiet, < 1 gates quiet, 1 = passthrough. Default 4 */
    upwardRatio?: Poly<Signal>;
    /** envelope attack in seconds (default 0.003) */
    attack?: Poly<Signal>;
    /** envelope release in seconds (default 0.05) */
    release?: Poly<Signal>;
    /** per-band makeup gain as dB-voltage (-5V = -24dB, 0V = unity, +5V = +24dB, default 1V ≈ +4.8dB) */
    makeup?: Poly<Signal>;
    /** low-band trim — 5 = unity (default 5) */
    lowGain?: Poly<Signal>;
    /** mid-band trim — 5 = unity (default 5) */
    midGain?: Poly<Signal>;
    /** high-band trim — 5 = unity (default 5) */
    highGain?: Poly<Signal>;
    id?: string;
}): Collection;

/**
 * @param count - Size of the output
 * @param playhead - 0..1 position (wraps), e.g. an LFO into \`.range(0, 1)\`
 * @param range - \`[off, on]\` weight pair (default \`[0, 5]\`, 5 = unity)
 * @param interpolationType - Easing between keyframes (default linear)
 *
 * @example
 * // Crossfade the amplitude of the different voices
 * const osc = $sine(['c', 'e', 'g'])
 * const weights = $cross(osc.length, $sine('0.25hz').range(0, 1));
 * osc.amp(weights).out();
 */
function $cross(
    count: number,
    playhead: Mono<Signal>,
    range?: [number, number],
    interpolationType?: Parameters<typeof $track>[1]['interpolationType'],
): Collection;

/**
 * Phase-warp table descriptors for modules that accept a {@link Table}
 * (e.g. the \`phase\` config field on \`$wavetable\`).
 *
 * Each helper returns a {@link Table} whose inner signal-valued field
 * accepts a constant, a module output, or any other {@link Signal}.
 *
 * Tables compose via the optional second argument. \`.pipe(fn)\` passes
 * the table to \`fn\` and returns the result — same API as \`Collection.pipe\`.
 *
 * @example
 * // Compose two tables (mirror feeds into bend):
 * $table.mirror(0.5, $table.bend(0.3))
 *
 * // Compose three tables left-to-right:
 * $table.mirror(0.5, $table.bend(0.3, $table.fold(0.2)))
 *
 * // Generic function application via .pipe:
 * const addBend = (t) => $table.bend(0.3, t)
 * $table.mirror(0.5).pipe(addBend)
 */
declare const $table: {
    /** Reflect the phase around its midpoint by \`amount\` (0..1). */
    mirror(amount: Poly<Signal>, next?: Table): Table;
    /** Bend the phase curve by \`amount\` (0..1 = linear..extreme). */
    bend(amount: Poly<Signal>, next?: Table): Table;
    /** Hard-sync: restart the phase every \`ratio\` of a cycle. */
    sync(ratio: Poly<Signal>, next?: Table): Table;
    /** Fold the phase back on itself by \`amount\`. */
    fold(amount: Poly<Signal>, next?: Table): Table;
    /** Pulse-width modulation warp with duty cycle \`width\` (0..1). */
    pwm(width: Poly<Signal>, next?: Table): Table;
};
`;

function generateWavsTypeDeclaration(tree: WavsFolderNode | null): string {
    if (!tree || Object.keys(tree).length === 0) {
        return '/** Load WAV samples from the wavs/ folder. */\nexport function $wavs(): Record<string, never>;\n';
    }

    function renderNode(node: WavsFolderNode, indent: string): string {
        const sorted = Object.entries(node).sort(([a], [b]) =>
            a.localeCompare(b),
        );
        const hasFiles = sorted.some(([, v]) => v === 'file');
        const lines: string[] = ['{'];
        // Numeric index signature (wraps modulo file count) when this folder
        // has at least one direct file. Out-of-range integers are valid at
        // runtime since access wraps; folders with zero files omit the
        // signature so `$wavs().emptyDir[0]` is a static type error.
        if (hasFiles) {
            lines.push(`${indent}  readonly [index: number]: WavHandle;`);
        }
        for (const [key, value] of sorted) {
            const safeName = /^[a-zA-Z_$][a-zA-Z0-9_$]*$/.test(key)
                ? key
                : `'${key.replace(/\\/g, '\\\\').replace(/'/g, "\\'")}'`;
            if (value === 'file') {
                lines.push(`${indent}  readonly ${safeName}: WavHandle;`);
            } else {
                lines.push(
                    `${indent}  readonly ${safeName}: ${renderNode(value, indent + '  ')}`,
                );
            }
        }
        lines.push(`${indent}}`);
        return lines.join('\n');
    }

    const treeType = renderNode(tree, '');
    return `/** Load WAV samples from the wavs/ folder. */\nexport function $wavs(): ${treeType};\n`;
}

export function buildLibSource(
    schemas: Schemas,
    wavsFolderTree?: WavsFolderNode | null,
): string {
    const schemaLib = generateDSL(schemas);
    const wavsDecl = generateWavsTypeDeclaration(wavsFolderTree ?? null);
    return `/* oxlint-disable */\ndeclare global {\n${BASE_LIB_SOURCE}\n\n${schemaLib}\n\n${wavsDecl}\n}\n\nexport {};\n`;
}

interface NamespaceNode {
    namespaces: Map<string, NamespaceNode>;
    classes: Map<string, Schema>;
    order: Array<{ kind: 'namespace' | 'class'; name: string }>;
}

function makeNamespaceNode(): NamespaceNode {
    return {
        classes: new Map(),
        namespaces: new Map(),
        order: [],
    };
}

function buildTreeFromSchemas(schemas: Schemas): NamespaceNode {
    const root = makeNamespaceNode();

    for (const moduleSchema of schemas) {
        const fullName = String(moduleSchema.name).trim();
        if (!fullName) {
            throw new Error('ModuleSchema is missing a non-empty name');
        }

        const { paramsSchema } = moduleSchema;
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

        node.classes.set(className, moduleSchema);
        node.order.push({ kind: 'class', name: className });
    }

    return root;
}

function capitalizeName(name: string): string {
    if (!name) {
        return name;
    }
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
function getMultiOutputInterfaceName(moduleSchema: Schema): string {
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
    moduleSchema: Schema,
    indent: string,
): string[] {
    const outputs = moduleSchema.outputs || [];
    if (outputs.length <= 1) {
        return [];
    }

    // Find the default output
    const defaultOutput = outputs.find((o) => o.default) || outputs[0];
    const baseType = getOutputType(defaultOutput);

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
        if (output.name === defaultOutput.name) {
            continue;
        }

        const outputType = getOutputType(output);
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
function getFactoryReturnType(moduleSchema: Schema): string {
    const outputs = moduleSchema.outputs || [];

    if (outputs.length === 0) {
        return 'void';
    } else if (outputs.length === 1) {
        return getOutputType(outputs[0]);
    }

    return getMultiOutputInterfaceName(moduleSchema);
}

/**
 * Build the trailing `config?: { ... }` argument shared by the factory-function
 * and `.$.`-method renderers: every non-positional param, plus an optional
 * `id`. `config` is required only when some non-positional param is required.
 * Returns the rendered argument and the nested `@param config` doc lines.
 */
function buildConfigArg(moduleSchema: Schema): {
    arg: string;
    paramDocs: string[];
} {
    const { paramsSchema } = moduleSchema;
    const schemaProperties = paramsSchema.properties as
        | Record<string, JSONSchema | undefined>
        | undefined;
    const schemaRequired: readonly string[] = paramsSchema.required || [];
    const positionalKeys = new Set(
        (moduleSchema.positionalArgs || []).map((a) => a.name),
    );
    const allParamKeys = Object.keys(paramsSchema.properties || {});

    const configProps: string[] = [];
    const paramDocs: string[] = [];

    for (const key of allParamKeys) {
        if (positionalKeys.has(key)) {
            continue;
        }
        const propSchema = schemaProperties?.[key];
        if (!propSchema) {
            continue;
        }
        const type = schemaToTypeExpr(propSchema, paramsSchema);
        const optionalMark = schemaRequired.includes(key) ? '' : '?';
        configProps.push(`${key}${optionalMark}: ${type}`);

        // Collect config param descriptions
        const description = propSchema?.description;
        if (description) {
            const firstLine = description.split(/\r?\n/)[0];
            paramDocs.push(`${key} - ${firstLine}`);
        }

        // Append enum variant descriptions as sub-bullets
        const variants = getEnumVariants(propSchema, paramsSchema);
        if (variants && variants.some((v) => v.description)) {
            for (const v of variants) {
                const desc = v.description ? ` — ${v.description}` : '';
                paramDocs.push(`    - \`${v.value}\`${desc}`);
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
    return { arg: `config${configOptional}: ${configType}`, paramDocs };
}

/**
 * Build the parameter list and JSDoc lines shared by the factory-function and
 * `.$.`-method renderers. `positionalStart` drops leading positionals (1 for
 * `.$.`, whose receiver is injected as the first argument); `leadParams` are
 * prepended to the parameter list (the `.$m.` `mix` crossfade); `leadDocLines`
 * are prepended before the module's own documentation. The module's full
 * documentation and every `@param` are always included, so the factory and
 * `.$.` surfaces carry identical docs and cannot drift.
 */
function buildSignature(
    moduleSchema: Schema,
    opts: {
        positionalStart: number;
        leadParams?: { decl: string; doc: string }[];
        leadDocLines?: string[];
    },
): { args: string[]; docLines: string[] } {
    const { paramsSchema } = moduleSchema;
    const schemaProperties = paramsSchema.properties as
        | Record<string, JSONSchema | undefined>
        | undefined;
    const schemaRequired: readonly string[] = paramsSchema.required || [];
    const positionals = (moduleSchema.positionalArgs || []).slice(
        opts.positionalStart,
    );
    const requiredness = positionals.map((a) =>
        schemaRequired.includes(a.name),
    );

    const args: string[] = [];
    const docLines: string[] = [...(opts.leadDocLines ?? [])];
    if (moduleSchema.documentation) {
        docLines.push(...moduleSchema.documentation.split(/\r?\n/));
    }

    for (const lead of opts.leadParams ?? []) {
        args.push(lead.decl);
        docLines.push(lead.doc);
    }

    for (let i = 0; i < positionals.length; i++) {
        const arg = positionals[i];
        const propSchema = schemaProperties?.[arg.name];
        const type = propSchema
            ? schemaToTypeExpr(propSchema, paramsSchema)
            : 'any';

        if (requiredness[i]) {
            args.push(`${arg.name}: ${type}`);
        } else {
            // Stay before any required arg: emit `| undefined` rather than `?`.
            const allSubsequentOptional = requiredness
                .slice(i + 1)
                .every((r) => !r);
            args.push(
                allSubsequentOptional
                    ? `${arg.name}?: ${type}`
                    : `${arg.name}: ${type} | undefined`,
            );
        }

        const description = propSchema?.description;
        if (description) {
            const firstLine = description.split(/\r?\n/)[0];
            docLines.push(`@param ${arg.name} - ${firstLine}`);
        } else {
            docLines.push(`@param ${arg.name}`);
        }

        if (propSchema) {
            const variants = getEnumVariants(propSchema, paramsSchema);
            if (variants && variants.some((v) => v.description)) {
                for (const v of variants) {
                    const desc = v.description ? ` — ${v.description}` : '';
                    docLines.push(`  - \`${v.value}\`${desc}`);
                }
            }
        }
    }

    const { arg: configArg, paramDocs } = buildConfigArg(moduleSchema);
    args.push(configArg);
    if (paramDocs.length > 0) {
        docLines.push(`@param config - Configuration object`);
        for (const doc of paramDocs) {
            docLines.push(`  - ${doc}`);
        }
    } else {
        docLines.push(`@param config - Configuration object`);
    }

    return { args, docLines };
}

/**
 * Render one method of the `.$.` (or `.$m.` when `withMix`) chainable namespace
 * for `moduleSchema`: the module's factory with its first positional dropped
 * (it becomes the chained signal receiver). For `.$m.`, a required leading
 * `mix` signal crossfades dry/wet and the return type collapses to
 * `Collection`. Carries the module's full documentation, like the factory.
 *
 * Only called on schemas passing `qualifiesForDollarChain`, which guarantees
 * `dollarMethodName` yields a valid TS identifier for the interface member.
 */
function renderDollarMethod(
    moduleSchema: Schema,
    withMix: boolean,
    indent: string,
): string[] {
    const rawName = moduleSchema.name.split('.').pop()!;
    const methodName = dollarMethodName(moduleSchema.name);

    const leadDocLines = withMix
        ? [
              `Chain through \`${rawName}\`, crossfading the dry input against`,
              'the wet result by a leading `mix` signal (0 = dry, 5 = wet,',
              '2.5 = equal). Equivalent to `.pipeMix(s => ' +
                  `${rawName}(s, ...), mix)\`.`,
              '',
          ]
        : [
              `Chain through \`${rawName}\` with this signal as its first`,
              `argument. Equivalent to \`${rawName}(this, ...)\`.`,
              '',
          ];

    const leadParams = withMix
        ? [
              {
                  decl: 'mix: Poly<Signal>',
                  doc: '@param mix - Dry/wet crossfade as {@link Poly<Signal>}. 0 = dry input only, 5 = wet result only, 2.5 = equal.',
              },
          ]
        : [];

    const { args, docLines } = buildSignature(moduleSchema, {
        positionalStart: 1,
        leadParams,
        leadDocLines,
    });

    const returnType = withMix
        ? 'Collection'
        : getFactoryReturnType(moduleSchema);

    const lines: string[] = [`${indent}/**`];
    for (const line of docLines) {
        lines.push(`${indent} * ${line}`);
    }
    lines.push(`${indent} */`);
    lines.push(`${indent}${methodName}(${args.join(', ')}): ${returnType};`);
    return lines;
}

function renderFactoryFunction(
    moduleSchema: Schema,
    _interfaceName: string,
    indent: string,
): string[] {
    const functionName = moduleSchema.name.split('.').pop()!;
    const { args, docLines } = buildSignature(moduleSchema, {
        positionalStart: 0,
    });
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

function renderInterface(classSpec: Schema, indent: string): string[] {
    const lines: string[] = [];

    // Generate multi-output interface if needed
    const multiOutputInterface = generateMultiOutputInterface(
        classSpec,
        indent,
    );
    if (multiOutputInterface.length > 0) {
        lines.push(...multiOutputInterface);
        lines.push('');
    }

    // Render the factory function
    lines.push(...renderFactoryFunction(classSpec, '', indent));
    return lines;
}

function renderTree(node: NamespaceNode, indentLevel: number = 0): string[] {
    const indent = '  '.repeat(indentLevel);
    const lines: string[] = [];

    for (const item of node.order) {
        if (item.kind === 'class') {
            const classSpec = node.classes.get(item.name);
            if (!classSpec) {
                continue;
            }
            lines.push(...renderInterface(classSpec, indent));
            lines.push('');
            continue;
        }

        const child = node.namespaces.get(item.name);
        if (!child) {
            continue;
        }
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

export function generateDSL(schemas: Schemas): string {
    // Filter out _clock (internal only) and $buffer (has a custom declaration below)
    const userFacingSchemas = schemas.filter(
        (s) => s.name !== '_clock' && s.name !== '$buffer',
    );
    const tree = buildTreeFromSchemas(userFacingSchemas);
    const lines = renderTree(tree, 0);

    // $clock is a pre-configured clock instance available to users.
    // Because the _clock factory is filtered from userFacingSchemas, its multi-output
    // Interface won't have been generated by renderTree. Generate it here
    // So that `$clock` has a proper type.
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

    lines.push('');
    lines.push(
        '/** Create a buffer module that captures an input signal into a circular audio buffer. */',
    );
    lines.push(
        'export function $buffer(input: ModuleOutput | Collection | number, lengthSeconds: number, config?: { id?: string }): BufferOutputRef;',
    );
    lines.push('');
    // Shared inline options type for `$delay`, `.$.delay`, and `.$m.delay`.
    const delayOpts =
        'opts?: { feedback?: Poly<Signal>; feedbackCb?: (mixed: Collection) => Collection | ModuleOutput; maxTime?: number }';
    // Shared documentation body for `$delay` / `.$.delay` / `.$m.delay`: all
    // three render the same body, and the chain forms prepend a header for
    // their call convention. These are synthetic chain methods, so the doc is
    // hand-written rather than schema-derived.
    const delayBody = [
        'Delay with feedback: sum the input with the attenuated feedback,',
        'optionally process that mix through `feedbackCb`, write it into a',
        'buffer of `maxTime` seconds, and read it back `time` seconds late. The',
        'delayed read is attenuated by `feedback` and summed back in, so a',
        '`feedbackCb` filter colours every recirculation. With `feedback` 0 you',
        'still hear the first echo; higher values repeat it.',
        '',
        '`feedback` is 0-5 (5 = unity), clamped, default 2.5. `maxTime` (default',
        '5) sizes the buffer and caps how long `time` can be. Returns the wet',
        'signal with the captured `buffer` attached for extra taps.',
        '',
        '```js',
        '// mix the dry signal back in for a classic echo',
        "let src = $saw('c3')",
        '$mix([src, $delay(src, 0.25, { feedback: 3 })]).out()',
        '',
        '// lowpass in the loop — echoes darken on every repeat',
        "src.$.delay(0.5, { feedbackCb: (m) => m.$.lpf('800hz') }).out()",
        '```',
    ];
    // Render the `$delay` doc at `indent`, prepending optional `header` lines
    // (a chain-form intro) before the shared body.
    const renderDelayDoc = (indent: string, header: string[]): string[] => {
        const star = `${indent} *`;
        const line = (text: string) => (text === '' ? star : `${star} ${text}`);
        const out = [`${indent}/**`, ...header.map(line)];
        if (header.length > 0) out.push(star);
        out.push(...delayBody.map(line), `${indent} */`);
        return out;
    };
    // Chain-form headers, mirroring `renderDollarMethod`'s lead docs.
    const dollarDelayHeader = [
        'Chain through `$delay` with this signal as its first argument.',
        'Equivalent to `$delay(this, time, opts)`.',
    ];
    const dollarMixDelayHeader = [
        'Chain through `$delay`, crossfading the dry input against the wet',
        'result by a leading `mix` signal (0 = dry, 5 = wet, 2.5 = equal).',
        'Equivalent to `.pipeMix(s => $delay(s, time, opts), mix)`.',
    ];
    lines.push(...renderDelayDoc('', []));
    lines.push(
        `export function $delay(input: Collection | ModuleOutput, time: Poly<Signal>, ${delayOpts}): Collection & { buffer: BufferOutputRef };`,
    );

    // `.$.` / `.$m.` chainable module namespaces. The qualifying set matches the
    // runtime `dollarLookup` exactly (shared `qualifiesForDollarChain` predicate
    // over the same schemas), so the two cannot drift.
    const dollarSchemas = userFacingSchemas.filter((s) =>
        qualifiesForDollarChain(
            processModuleSchema(s as unknown as ModuleSchema),
        ),
    );

    // Flat modules (`$lpf`) become direct members; dotted modules
    // (`$unstable.shape.fold`) nest under a namespace tree of any depth
    // (`.$.unstable.shape.fold`), mirroring the global namespace and keeping leaf
    // names like `fold` from colliding with flat modules like `$fold`.
    interface DollarTypeNode {
        leaves: Schema[];
        children: Map<string, DollarTypeNode>;
    }
    const dollarRoot: DollarTypeNode = { leaves: [], children: new Map() };
    for (const s of dollarSchemas) {
        const base = s.name.startsWith('$') ? s.name.slice(1) : s.name;
        const segments = base.split('.');
        let node = dollarRoot;
        for (const seg of segments.slice(0, -1)) {
            let child = node.children.get(seg);
            if (child === undefined) {
                child = { leaves: [], children: new Map() };
                node.children.set(seg, child);
            }
            node = child;
        }
        node.leaves.push(s);
    }
    const pascal = (s: string): string =>
        s
            .split(/[^a-zA-Z0-9]/)
            .filter(Boolean)
            .map((w) => w[0].toUpperCase() + w.slice(1))
            .join('');
    const sortedChildren = (node: DollarTypeNode): string[] =>
        [...node.children.keys()].sort();

    // Emit a sub-interface (dry and mix) for every namespace node, so
    // `.$.unstable.shape.fold` yields `DollarChainUnstable { shape:
    // DollarChainUnstableShape }` and `DollarChainUnstableShape { fold(...) }`.
    const emitNamespaceInterfaces = (
        node: DollarTypeNode,
        path: string[],
    ): void => {
        for (const seg of sortedChildren(node)) {
            emitNamespaceInterfaces(node.children.get(seg)!, [...path, seg]);
        }
        const dotted = path.join('.');
        for (const [prefix, iface, withMix] of [
            ['.$', 'DollarChain', false],
            ['.$m', 'DollarMixChain', true],
        ] as const) {
            lines.push('');
            lines.push(
                `/** Methods of the \`${prefix}.${dotted}\` chainable namespace. */`,
            );
            lines.push(`interface ${iface}${pascal(dotted)} {`);
            for (const s of node.leaves) {
                lines.push(...renderDollarMethod(s, withMix, '  '));
            }
            for (const seg of sortedChildren(node)) {
                lines.push(
                    `  ${seg}: ${iface}${pascal([...path, seg].join('.'))};`,
                );
            }
            lines.push('}');
        }
    };
    for (const seg of sortedChildren(dollarRoot)) {
        emitNamespaceInterfaces(dollarRoot.children.get(seg)!, [seg]);
    }

    lines.push('');
    lines.push(
        '/** Methods of the `.$` chainable module namespace (see {@link ModuleOutput.$}). */',
    );
    lines.push('interface DollarChain {');
    for (const s of dollarRoot.leaves) {
        lines.push(...renderDollarMethod(s, false, '  '));
    }
    for (const seg of sortedChildren(dollarRoot)) {
        lines.push(`  ${seg}: DollarChain${pascal(seg)};`);
    }
    lines.push(
        ...renderDelayDoc('  ', dollarDelayHeader),
        `  delay(time: Poly<Signal>, ${delayOpts}): Collection & { buffer: BufferOutputRef };`,
    );
    lines.push('}');

    lines.push('');
    lines.push(
        '/** Methods of the `.$m` chainable module namespace (see {@link ModuleOutput.$m}). */',
    );
    lines.push('interface DollarMixChain {');
    for (const s of dollarRoot.leaves) {
        lines.push(...renderDollarMethod(s, true, '  '));
    }
    for (const seg of sortedChildren(dollarRoot)) {
        lines.push(`  ${seg}: DollarMixChain${pascal(seg)};`);
    }
    lines.push(
        ...renderDelayDoc('  ', dollarMixDelayHeader),
        `  delay(mix: Poly<Signal>, time: Poly<Signal>, ${delayOpts}): Collection;`,
    );
    lines.push('}');

    return lines.join('\n') + '\n';
}
