import { describe, expect, it } from 'vitest';
import { generateVideoNamespace } from '../videoLibGen';

describe('generateVideoNamespace', () => {
    it('gives every line inside a JSDoc block its gutter, multi-line examples included', () => {
        const generated = generateVideoNamespace();
        const blocks = [...generated.matchAll(/\/\*\*\n([\s\S]*?)\n\s*\*\//g)];
        expect(blocks.length).toBeGreaterThan(50);
        for (const [, body] of blocks) {
            for (const line of body.split('\n')) {
                expect(line).toMatch(/^\s* \*( |$)/);
            }
        }
        expect(generated).toMatch(
            /\* @example const clip = .*\n\s+\* clip\.out\(\);/,
        );
    });
});
