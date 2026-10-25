import { describe, expect, it } from 'vitest';
import {
    combineReleaseNotes,
    compareVersions,
    hasNotes,
    isBreakingVersionBump,
    isNewerVersion,
    mergeReleaseNotes,
    parseChangelog,
    parseReleaseBody,
    parseVersion,
    releasesInRange,
    updateHasBreakingChanges,
    whatsNewSince,
    type ReleaseNotes,
} from './releaseNotes';

const BODY = [
    '### Breaking',
    '',
    '- `$foo` is now called `$bar`. Update your patches:',
    '  ',
    '  ```js',
    "  $bar('c4')",
    '  ```',
    '',
    '### Fixed',
    '',
    '- Scopes no longer flicker when you resize the window.',
    '- Saving works offline.',
    '',
].join('\n');

const CHANGELOG = [
    '# Changelog',
    '',
    '## v0.2.0 — November 2, 2026',
    '',
    '### New',
    '',
    '- A reverb.',
    '',
    '## v0.1.1 — October 30, 2026',
    '',
    'Maintenance release.',
    '',
    '## v0.1.0 — October 24, 2026',
    '',
    BODY,
    '## v0.0.130',
    '',
    'Releases up to and including 0.0.130 have no release notes.',
    '',
].join('\n');

function release(
    version: string,
    sections: ReleaseNotes['sections'],
): ReleaseNotes {
    return { date: null, sections, version };
}

describe('parseVersion / compareVersions', () => {
    it('accepts plain and v-prefixed versions', () => {
        expect(parseVersion('0.1.0')).toEqual([0, 1, 0]);
        expect(parseVersion('v1.20.3')).toEqual([1, 20, 3]);
    });

    it('rejects prereleases and malformed input', () => {
        expect(parseVersion('1.0.0-beta.1')).toBeNull();
        expect(parseVersion('1.0')).toBeNull();
        expect(parseVersion('')).toBeNull();
    });

    it('compares numerically per component', () => {
        expect(compareVersions('0.0.130', '0.0.99')).toBeGreaterThan(0);
        expect(compareVersions('0.1.0', '0.0.130')).toBeGreaterThan(0);
        expect(compareVersions('v1.0.0', '1.0.0')).toBe(0);
        expect(compareVersions('0.9.9', '1.0.0')).toBeLessThan(0);
        expect(compareVersions('nope', '1.0.0')).toBeNaN();
    });

    it('isNewerVersion is false for unparseable versions', () => {
        expect(isNewerVersion('0.1.0', '0.0.130')).toBe(true);
        expect(isNewerVersion('0.1.0', '0.1.0')).toBe(false);
        expect(isNewerVersion('2.0.0-rc.1', '1.0.0')).toBe(false);
    });

    it('treats a minor bump before 1.0 and a major bump as breaking', () => {
        expect(isBreakingVersionBump('0.0.130', '0.1.0')).toBe(true);
        expect(isBreakingVersionBump('0.1.0', '0.1.5')).toBe(false);
        expect(isBreakingVersionBump('1.2.0', '1.3.0')).toBe(false);
        expect(isBreakingVersionBump('1.9.0', '2.0.0')).toBe(true);
        expect(isBreakingVersionBump('0.2.0', '0.1.0')).toBe(false);
    });
});

describe('parseReleaseBody', () => {
    it('splits entries into kinds and keeps multi-line code fences', () => {
        const sections = parseReleaseBody(BODY);
        expect(sections.Breaking).toEqual([
            "`$foo` is now called `$bar`. Update your patches:\n\n```js\n$bar('c4')\n```",
        ]);
        expect(sections.Fixed).toEqual([
            'Scopes no longer flicker when you resize the window.',
            'Saving works offline.',
        ]);
        expect(sections.New).toBeUndefined();
    });

    it('handles CRLF line endings', () => {
        expect(parseReleaseBody('### New\r\n\r\n- A reverb.\r\n').New).toEqual([
            'A reverb.',
        ]);
    });

    it('does not split on bullets inside a code fence', () => {
        const sections = parseReleaseBody(
            [
                '### Improved',
                '',
                '- Lists:',
                '```',
                '- not an entry',
                '```',
            ].join('\n'),
        );
        expect(sections.Improved).toEqual(['Lists:\n```\n- not an entry\n```']);
    });

    it('finds no notes in bodies without kind sections', () => {
        for (const body of [
            '',
            'Releases up to and including 0.0.130 have no release notes.',
            '## What changed\n\n- something',
            '### Other\n\n- unknown kind',
        ]) {
            expect(hasNotes(release('1.0.0', parseReleaseBody(body)))).toBe(
                false,
            );
        }
    });
});

describe('parseChangelog', () => {
    it('reads each version section with its date', () => {
        const releases = parseChangelog(CHANGELOG);
        expect(releases.map((r) => [r.version, r.date])).toEqual([
            ['0.2.0', 'November 2, 2026'],
            ['0.1.1', 'October 30, 2026'],
            ['0.1.0', 'October 24, 2026'],
            ['0.0.130', null],
        ]);
        expect(releases[0].sections.New).toEqual(['A reverb.']);
        expect(releases[2].sections).toEqual(parseReleaseBody(BODY));
        expect(hasNotes(releases[1])).toBe(false);
        expect(hasNotes(releases[3])).toBe(false);
    });

    it('parses the changelog shipped with the app', async () => {
        const fs = await import('fs');
        const path = await import('path');
        const text = fs.readFileSync(
            path.resolve(__dirname, '../../CHANGELOG.md'),
            'utf8',
        );
        const releases = parseChangelog(text);
        expect(releases.length).toBeGreaterThan(0);
        expect(releases.every((r) => parseVersion(r.version))).toBe(true);
    });
});

describe('releasesInRange / whatsNewSince', () => {
    const changelog = parseChangelog(CHANGELOG);

    it('keeps (after, upTo] with notes, newest first', () => {
        expect(
            releasesInRange(changelog, '0.0.130', '0.2.0').map(
                (r) => r.version,
            ),
        ).toEqual(['0.2.0', '0.1.0']);
        expect(
            releasesInRange(changelog, '0.1.0', '0.1.1').map((r) => r.version),
        ).toEqual([]);
        expect(
            releasesInRange(changelog, '0.0.1', '0.1.0').map((r) => r.version),
        ).toEqual(['0.1.0']);
    });

    it('sorts out-of-order input', () => {
        const shuffled = [changelog[2], changelog[0], changelog[1]];
        expect(
            releasesInRange(shuffled, '0.0.0', '9.9.9').map((r) => r.version),
        ).toEqual(['0.2.0', '0.1.0']);
    });

    it('shows nothing on a fresh install', () => {
        expect(whatsNewSince(changelog, undefined, '0.2.0')).toEqual([]);
    });

    it('shows nothing when the version has not changed', () => {
        expect(whatsNewSince(changelog, '0.2.0', '0.2.0')).toEqual([]);
    });

    it('covers every release after the last-seen version', () => {
        expect(
            whatsNewSince(changelog, '0.0.130', '0.2.0').map((r) => r.version),
        ).toEqual(['0.2.0', '0.1.0']);
    });
});

describe('combineReleaseNotes / updateHasBreakingChanges', () => {
    const notes = [
        release('0.3.0', { Breaking: ['B3'], Fixed: ['F3'] }),
        release('0.2.0', { Breaking: ['B2a', 'B2b'] }),
        release('0.1.1', { New: ['N1'] }),
    ];

    it('lifts breaking changes to the top with their versions', () => {
        const combined = combineReleaseNotes(notes);
        expect(combined.breaking).toEqual([
            { entry: 'B3', version: '0.3.0' },
            { entry: 'B2a', version: '0.2.0' },
            { entry: 'B2b', version: '0.2.0' },
        ]);
        expect(combined.releases).toEqual([
            release('0.3.0', { Fixed: ['F3'] }),
            release('0.1.1', { New: ['N1'] }),
        ]);
    });

    it('flags breaking changes from notes', () => {
        expect(updateHasBreakingChanges(notes, '0.1.0', '0.3.0')).toBe(true);
        expect(updateHasBreakingChanges([notes[2]], '0.0.130', '0.1.1')).toBe(
            false,
        );
    });

    it('falls back to the version bump when there are no notes', () => {
        expect(updateHasBreakingChanges([], '0.0.130', '0.1.0')).toBe(true);
        expect(updateHasBreakingChanges([], '0.0.129', '0.0.130')).toBe(false);
    });
});

describe('mergeReleaseNotes', () => {
    const bundled = [
        release('0.1.1', { Fixed: ['bundled 0.1.1'] }),
        release('0.1.0', { New: ['bundled 0.1.0'] }),
        release('0.0.130', {}),
    ];

    it('adds published releases newer than the bundled changelog', () => {
        const merged = mergeReleaseNotes(
            [
                release('0.2.0', { New: ['published 0.2.0'] }),
                release('0.1.1', { Fixed: ['published 0.1.1'] }),
            ],
            bundled,
        );
        expect(merged.map((r) => r.version)).toEqual([
            '0.2.0',
            '0.1.1',
            '0.1.0',
        ]);
    });

    it('keeps one entry per version, preferring the published notes', () => {
        const merged = mergeReleaseNotes(
            [release('0.1.1', { Fixed: ['published 0.1.1'] })],
            bundled,
        );
        expect(merged.find((r) => r.version === '0.1.1')?.sections).toEqual({
            Fixed: ['published 0.1.1'],
        });
    });

    it('keeps bundled notes when the published release has none', () => {
        const merged = mergeReleaseNotes([release('0.1.0', {})], bundled);
        expect(merged.find((r) => r.version === '0.1.0')?.sections).toEqual({
            New: ['bundled 0.1.0'],
        });
    });

    it('falls back to the bundled changelog alone', () => {
        expect(mergeReleaseNotes([], bundled).map((r) => r.version)).toEqual([
            '0.1.1',
            '0.1.0',
        ]);
    });
});
