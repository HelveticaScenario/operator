/**
 * Fields of the option objects taken by `out()` and `outMono()`. The generated
 * DSL lib declares the interfaces from these and lists them in the methods'
 * own JSDoc, and the help window renders the same fields.
 */

export interface OutOptionField {
    name: string;
    type: string;
    doc: string;
}

const LEVEL_FIELDS: readonly OutOptionField[] = [
    {
        doc: 'Label shown on this output\'s VU meter. Must be unique across outs',
        name: 'label',
        type: 'string',
    },
    {
        doc: 'Silence this output. It is still metered; its VU meter greys out',
        name: 'mute',
        type: 'boolean',
    },
    {
        doc: 'When any output is soloed, only soloed outputs are audible',
        name: 'solo',
        type: 'boolean',
    },
];

export const STEREO_OUT_OPTIONS: readonly OutOptionField[] = [
    {
        doc: 'Base output channel (0-62, default 0). Left plays on baseChannel, right on baseChannel+1',
        name: 'baseChannel',
        type: 'number',
    },
    {
        doc: 'Output gain. If set, a $scaleAndShift module is added after the stereo mix',
        name: 'gain',
        type: 'Poly<Signal>',
    },
    {
        doc: 'Pan position (-5 = left, 0 = center, +5 = right). Default 0',
        name: 'pan',
        type: 'Poly<Signal>',
    },
    {
        doc: 'Stereo width/spread (0 = no spread, 5 = full spread). Default 0',
        name: 'width',
        type: 'Mono<Signal>',
    },
    ...LEVEL_FIELDS,
];

export const MONO_OUT_OPTIONS: readonly OutOptionField[] = [
    {
        doc: 'Output channel (0-63, default 0). Wins over the positional channel argument',
        name: 'channel',
        type: 'number',
    },
    {
        doc: 'Output gain. If set, a $scaleAndShift module is added after the mix',
        name: 'gain',
        type: 'Poly<Signal>',
    },
    ...LEVEL_FIELDS,
];

/** One-line type expression, e.g. `interface { label?: string; ... }`. */
export function outOptionsDefinition(fields: readonly OutOptionField[]): string {
    return `interface { ${fields.map((f) => `${f.name}?: ${f.type};`).join(' ')} }`;
}

/** Interface members with JSDoc, indented for the generated lib. */
export function renderOutOptionsMembers(
    fields: readonly OutOptionField[],
): string {
    return fields
        .map((f) => `  /** ${f.doc} */\n  ${f.name}?: ${f.type};`)
        .join('\n');
}

/**
 * Markdown list of the fields as JSDoc body lines, for a method whose hover
 * should show the options without following the interface.
 */
export function renderOutOptionsJsDocList(
    fields: readonly OutOptionField[],
): string {
    return fields
        .map((f) => `   * - \`${f.name}\` (\`${f.type}\`): ${f.doc}`)
        .join('\n');
}
