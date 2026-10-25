import { describe, expect, test } from 'vitest';
import schemas from '@modular/core/schemas.json';
import { buildLibSource } from '../typescriptLibGen';
import { EXPERIMENTAL_NOTICE } from '../../../shared/dsl/experimental';

/** The JSDoc block directly above the declaration matching `declaration`. */
function jsDocAbove(source: string, declaration: RegExp): string {
    const match = declaration.exec(source);
    expect(match, `no declaration matching ${declaration}`).not.toBeNull();
    const before = source.slice(0, match!.index);
    return before.slice(before.lastIndexOf('/**'));
}

describe('experimental module docs', () => {
    const source = buildLibSource(schemas);
    const experimental = schemas.filter((s) => s.experimental);

    test('the schemas mark at least one module experimental', () => {
        expect(experimental.map((s) => s.name)).toContain('$comp');
    });

    test('an experimental factory leads with the notice and carries the tag', () => {
        const doc = jsDocAbove(source, /export function \$comp\(/);
        expect(doc.split('\n')[1].trim()).toBe(`* ${EXPERIMENTAL_NOTICE}`);
        expect(doc).toMatch(/\* @experimental\n/);
    });

    test('its `.$.` method carries the same marker', () => {
        const doc = jsDocAbove(source, /^\s*comp\(/m);
        expect(doc.split('\n')[1].trim()).toBe(`* ${EXPERIMENTAL_NOTICE}`);
        expect(doc).toMatch(/\* @experimental\n/);
    });

    test('a stable module has neither', () => {
        const doc = jsDocAbove(source, /export function \$sine\(/);
        expect(doc).not.toContain(EXPERIMENTAL_NOTICE);
        expect(doc).not.toContain('@experimental');
    });
});
