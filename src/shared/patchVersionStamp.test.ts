import { describe, expect, it } from 'vitest';
import {
    isStampablePath,
    stampPatchVersionSource,
    stripPatchVersionStamp,
} from './patchVersionStamp';

const PATCH = `// Simple 440 Hz sine wave\n$sine('a3').out();\n`;

describe('isStampablePath', () => {
    it('accepts JavaScript patch extensions, case-insensitively', () => {
        expect(isStampablePath('foo.js')).toBe(true);
        expect(isStampablePath('foo.mjs')).toBe(true);
        expect(isStampablePath('/abs/Path/Bar.MJS')).toBe(true);
    });

    it('rejects non-patch files', () => {
        expect(isStampablePath('keybindings.json')).toBe(false);
        expect(isStampablePath('sample.wav')).toBe(false);
        expect(isStampablePath('README.md')).toBe(false);
        expect(isStampablePath('mjs')).toBe(false);
    });
});

describe('stampPatchVersionSource', () => {
    it('prepends a metadata block above the patch body', () => {
        const stamped = stampPatchVersionSource(PATCH, '0.0.101');
        expect(stamped).toBe(
            `/**\n * @operator\n * @version 0.0.101\n */\n\n${PATCH}`,
        );
    });

    it('round-trips: strip undoes stamp exactly', () => {
        expect(
            stripPatchVersionStamp(stampPatchVersionSource(PATCH, '1.2.3')),
        ).toBe(PATCH);
    });

    it('is idempotent — re-stamping replaces rather than stacks blocks', () => {
        const once = stampPatchVersionSource(PATCH, '0.0.1');
        const twice = stampPatchVersionSource(once, '0.0.1');
        expect(twice).toBe(once);
    });

    it('updates the version on re-stamp without leaving the old one', () => {
        const old = stampPatchVersionSource(PATCH, '0.0.1');
        const next = stampPatchVersionSource(old, '0.0.2');
        expect(next).toContain('@version 0.0.2');
        expect(next).not.toContain('@version 0.0.1');
        expect(next.match(/@operator/g)).toHaveLength(1);
    });

    it('handles an empty patch', () => {
        const stamped = stampPatchVersionSource('', '0.0.1');
        expect(stamped).toBe(`/**\n * @operator\n * @version 0.0.1\n */\n`);
        expect(stripPatchVersionStamp(stamped)).toBe('');
    });
});

describe('stripPatchVersionStamp', () => {
    it('leaves an unstamped patch untouched (backward compatible)', () => {
        expect(stripPatchVersionStamp(PATCH)).toBe(PATCH);
    });

    it("leaves a user's own leading JSDoc untouched", () => {
        const withDoc = `/**\n * My cool patch\n * @author me\n */\n\n${PATCH}`;
        expect(stripPatchVersionStamp(withDoc)).toBe(withDoc);
    });

    it('does not treat @operator inside prose or an email as the block', () => {
        const prose = `// see @operatorX docs\n${PATCH}`;
        expect(stripPatchVersionStamp(prose)).toBe(prose);
        const email = `/* contact me@operator.com */\n${PATCH}`;
        expect(stripPatchVersionStamp(email)).toBe(email);
    });

    it('strips a leading BOM-prefixed block', () => {
        const stamped = `\uFEFF${stampPatchVersionSource(PATCH, '0.0.1')}`;
        expect(stripPatchVersionStamp(stamped)).toBe(PATCH);
    });

    it('tolerates a hand-edited block with a single trailing newline', () => {
        const stamped = `/**\n * @operator\n * @version 0.0.1\n */\n${PATCH}`;
        expect(stripPatchVersionStamp(stamped)).toBe(PATCH);
    });

    it('handles CRLF line endings around the block', () => {
        const body = `line1\r\nline2\r\n`;
        const stamped = stampPatchVersionSource(body, '0.0.1');
        expect(stripPatchVersionStamp(stamped)).toBe(body);
    });
});
