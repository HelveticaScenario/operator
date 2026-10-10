import {
    VIDEO_CHAIN,
    VIDEO_CHAIN_EXAMPLES,
    VIDEO_DOCS,
    VIDEO_INTRO,
    type VideoDocParam,
} from '../../shared/dsl/videoDocs';

const WRAP_WIDTH = 76;

/** Breaks `text` into lines of at most `width` characters, keeping paragraph breaks. */
function wrapText(text: string, width: number): string[] {
    const lines: string[] = [];
    text.split('\n\n').forEach((paragraph, i) => {
        if (i > 0) lines.push('');
        let line = '';
        for (const word of paragraph.split(' ')) {
            if (line !== '' && line.length + 1 + word.length > width) {
                lines.push(line);
                line = word;
            } else {
                line = line === '' ? word : `${line} ${word}`;
            }
        }
        lines.push(line);
    });
    return lines;
}

function jsdoc(
    indent: string,
    description: string,
    params: VideoDocParam[],
    examples: string[],
): string {
    const width = WRAP_WIDTH - indent.length;
    const lines = wrapText(description, width);
    for (const { name, description: text } of params) {
        const [first, ...rest] = wrapText(`@param ${name} ${text}`, width);
        lines.push(first, ...rest.map((line) => `  ${line}`));
    }
    for (const example of examples) lines.push(`@example ${example}`);
    return [
        `${indent}/**`,
        ...lines.map((line) =>
            line === '' ? `${indent} *` : `${indent} * ${line}`,
        ),
        `${indent} */`,
    ].join('\n');
}

/** The `declare const $v` block with JSDoc, generated from {@link VIDEO_DOCS}. */
export function generateVideoNamespace(): string {
    const members = VIDEO_DOCS.map((doc) => {
        const declarations = doc.declarations
            .map((declaration) =>
                declaration
                    .split('\n')
                    .map((line) => `    ${line}`)
                    .join('\n'),
            )
            .join('\n');
        return `${jsdoc('    ', doc.description, doc.params, doc.examples)}\n${declarations}`;
    });
    return [
        jsdoc(
            '',
            VIDEO_INTRO.description,
            [],
            [...VIDEO_INTRO.examples, ...VIDEO_CHAIN_EXAMPLES],
        ),
        'declare const $v: {',
        ...members,
        '};',
    ].join('\n');
}

/** Declarations of the chain functions that apply to `kind`, with `{self}` filled in. */
function chainDeclarations(
    kind: 'field' | 'color',
    direct: boolean,
): { doc: string; declarations: string[] }[] {
    const self = kind === 'field' ? 'VideoField' : 'VideoColor';
    return VIDEO_CHAIN.filter(
        (doc) => (doc.direct ?? false) === direct,
    ).flatMap((doc) => {
        const declarations =
            doc[kind] ??
            ((doc.on ?? ['field', 'color']).includes(kind)
                ? doc.declarations
                : undefined);
        if (!declarations) return [];
        return [
            {
                declarations: declarations.map((declaration) =>
                    declaration.replace('{self}', self),
                ),
                doc: doc.description,
            },
        ];
    });
}

/** A chain declaration with a leading `mix` parameter, as `.$m` takes. */
function withMix(declaration: string): string {
    return declaration
        .replace(/^(\w+)\(\s*\)/, '$1(mix: VideoValue)')
        .replace(/^(\w+)\((?!mix: VideoValue\))/, '$1(mix: VideoValue, ');
}

function renderMembers(
    entries: { doc: string; declarations: string[] }[],
    transform: (declaration: string) => string = (d) => d,
): string {
    return entries
        .flatMap(({ doc, declarations }) => [
            jsdoc('    ', doc, [], []),
            ...declarations.map(
                (declaration) => `    ${transform(declaration)}`,
            ),
        ])
        .join('\n');
}

/**
 * The members a video signal has besides its type tag: the direct methods
 * that end or tap a chain, `$` and `$m`, `pipe` and `pipeMix`.
 */
export function generateVideoChainMembers(kind: 'field' | 'color'): string {
    const self = kind === 'field' ? 'VideoField' : 'VideoColor';
    const sig = kind === 'field' ? 'Field' : 'Color';
    const pipeMix =
        kind === 'field'
            ? `    pipeMix(fn: (self: VideoField) => VideoField, mix?: VideoValue): VideoField;
    pipeMix(fn: (self: VideoField) => VideoColor, mix?: VideoValue): VideoColor;`
            : `    pipeMix(fn: (self: VideoColor) => VideoField | VideoColor, mix?: VideoValue): VideoColor;`;
    return [
        '    /** The `$v` functions that take a signal first, applied to this signal: `x.$.rotate(0.1)`. */',
        `    readonly $: Video${sig}Chain;`,
        '    /** Like `$`, with a leading `mix` that crossfades this signal against the result: 0 is this signal, 1 the result. */',
        `    readonly $m: Video${sig}MixChain;`,
        '    /** Calls `fn` with this signal and returns what it returns. */',
        `    pipe<U>(fn: (self: ${self}) => U): U;`,
        '    /** Calls `fn(this, element)` for each element of `array` and returns the results. */',
        `    pipe<U, E>(fn: (self: ${self}, element: E) => U, array: E[]): U[];`,
        '    /** Crossfades this signal against `fn(this)`: 0 is this signal, 1 the result (default 0.5). */',
        pipeMix,
        renderMembers(chainDeclarations(kind, true)),
    ].join('\n');
}

/** The four `$` and `$m` namespace interfaces, generated from {@link VIDEO_CHAIN}. */
export function generateVideoChainInterfaces(): string {
    return (['field', 'color'] as const)
        .flatMap((kind) => {
            const sig = kind === 'field' ? 'Field' : 'Color';
            const entries = chainDeclarations(kind, false);
            return [
                `/** The functions of \`.$\` on a video ${kind}. */\ninterface Video${sig}Chain {\n${renderMembers(entries)}\n}`,
                `/** The functions of \`.$m\` on a video ${kind}: each takes a leading \`mix\`. */\ninterface Video${sig}MixChain {\n${renderMembers(entries, withMix)}\n}`,
            ];
        })
        .join('\n\n');
}
