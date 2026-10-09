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

/**
 * The chain methods of a video signal, as interface members with JSDoc,
 * generated from {@link VIDEO_CHAIN}.
 */
export function generateVideoChainMembers(kind: 'field' | 'color'): string {
    const self = kind === 'field' ? 'VideoField' : 'VideoColor';
    return VIDEO_CHAIN.flatMap((doc) => {
        const declarations =
            doc[kind] ??
            ((doc.on ?? ['field', 'color']).includes(kind)
                ? doc.declarations
                : undefined);
        if (!declarations) return [];
        return [
            jsdoc('    ', doc.description, [], []),
            ...declarations.map(
                (declaration) => `    ${declaration.replace('{self}', self)}`,
            ),
        ];
    }).join('\n');
}
