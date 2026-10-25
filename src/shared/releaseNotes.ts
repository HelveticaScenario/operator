/**
 * Release-note parsing shared by the update check (GitHub release bodies) and
 * the bundled CHANGELOG.md. Both carry the changie layout: `### <Kind>`
 * sections of markdown list items, with continuation lines indented by two
 * spaces.
 */

export const RELEASE_NOTE_KINDS = [
    'Breaking',
    'New',
    'Improved',
    'Fixed',
] as const;

export type ReleaseNoteKind = (typeof RELEASE_NOTE_KINDS)[number];

/** Each entry is the markdown body of one list item, without its bullet. */
export type ReleaseNoteSections = Partial<Record<ReleaseNoteKind, string[]>>;

export interface ReleaseNotes {
    /** Plain `X.Y.Z`, without a leading `v`. */
    version: string;
    /** Human-readable release date, when the source records one. */
    date: string | null;
    sections: ReleaseNoteSections;
}

export interface BreakingChange {
    version: string;
    entry: string;
}

/** Notes for a span of releases, with every breaking change pulled to the top. */
export interface CombinedReleaseNotes {
    /** Newest release first. */
    breaking: BreakingChange[];
    /** Newest release first; only releases with non-breaking entries. */
    releases: ReleaseNotes[];
}

type Version = [number, number, number];

/** Parses `X.Y.Z` or `vX.Y.Z`. Prerelease and build suffixes are rejected. */
export function parseVersion(text: string): Version | null {
    const match = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(text.trim());
    if (!match) {
        return null;
    }
    return [Number(match[1]), Number(match[2]), Number(match[3])];
}

/** Orders two parsed versions: negative when `a` is older than `b`. */
function compareParsed(a: Version, b: Version): number {
    return a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
}

/**
 * Orders two version strings: negative when `a` is older than `b`. Returns
 * `NaN` when either is not a plain `X.Y.Z` version.
 */
export function compareVersions(a: string, b: string): number {
    const pa = parseVersion(a);
    const pb = parseVersion(b);
    if (!pa || !pb) {
        return NaN;
    }
    return compareParsed(pa, pb);
}

/** True when `candidate` is strictly newer than `base`; false if either is unparseable. */
export function isNewerVersion(candidate: string, base: string): boolean {
    return compareVersions(candidate, base) > 0;
}

/**
 * True when moving from `from` to `to` crosses a breaking-change boundary:
 * a major bump, or before 1.0 a minor bump.
 */
export function isBreakingVersionBump(from: string, to: string): boolean {
    const a = parseVersion(from);
    const b = parseVersion(to);
    if (!a || !b || compareParsed(b, a) <= 0) {
        return false;
    }
    if (b[0] !== a[0]) {
        return true;
    }
    return b[0] === 0 && b[1] !== a[1];
}

function kindFromHeading(heading: string): ReleaseNoteKind | null {
    const lower = heading.trim().toLowerCase();
    return (
        RELEASE_NOTE_KINDS.find((kind) => kind.toLowerCase() === lower) ?? null
    );
}

/**
 * Splits a release body into its kinds. Text outside `### <Kind>` sections
 * and unknown headings are ignored, so a body without any kind sections has
 * no notes.
 */
export function parseReleaseBody(body: string): ReleaseNoteSections {
    const sections: ReleaseNoteSections = {};
    let kind: ReleaseNoteKind | null = null;
    let entry: string[] | null = null;
    let inFence = false;

    const finishEntry = () => {
        if (kind && entry) {
            const text = entry.join('\n').trim();
            if (text) {
                (sections[kind] ??= []).push(text);
            }
        }
        entry = null;
        inFence = false;
    };

    for (const line of body.replace(/\r\n?/g, '\n').split('\n')) {
        if (!inFence) {
            const heading = /^(#{1,3})\s+(.*)$/.exec(line);
            if (heading) {
                finishEntry();
                kind =
                    heading[1] === '###' ? kindFromHeading(heading[2]) : null;
                continue;
            }
            const bullet = /^[-*]\s+(.*)$/.exec(line);
            if (bullet && kind) {
                finishEntry();
                entry = [bullet[1]];
                continue;
            }
        }
        if (entry) {
            const continuation = line.replace(/^ {1,2}/, '');
            if (/^\s*(```|~~~)/.test(continuation)) {
                inFence = !inFence;
            }
            entry.push(continuation);
        }
    }
    finishEntry();
    return sections;
}

export function hasNotes(release: ReleaseNotes): boolean {
    return RELEASE_NOTE_KINDS.some(
        (kind) => (release.sections[kind]?.length ?? 0) > 0,
    );
}

/**
 * Parses CHANGELOG.md into one entry per `## vX.Y.Z — <date>` section, in
 * file order. Sections whose heading is not a plain version are skipped.
 */
export function parseChangelog(markdown: string): ReleaseNotes[] {
    const releases: ReleaseNotes[] = [];
    let current: { heading: string; lines: string[] } | null = null;

    const finish = () => {
        if (!current) {
            return;
        }
        const match = /^v?(\d+\.\d+\.\d+)(?:\s+—\s+(.+))?\s*$/.exec(
            current.heading.trim(),
        );
        if (match) {
            releases.push({
                date: match[2]?.trim() || null,
                sections: parseReleaseBody(current.lines.join('\n')),
                version: match[1],
            });
        }
    };

    for (const line of markdown.replace(/\r\n?/g, '\n').split('\n')) {
        const heading = /^##\s+(.*)$/.exec(line);
        if (heading) {
            finish();
            current = { heading: heading[1], lines: [] };
        } else if (current) {
            current.lines.push(line);
        }
    }
    finish();
    return releases;
}

/**
 * Releases newer than `after` up to and including `upTo` that have notes,
 * newest first.
 */
export function releasesInRange(
    releases: ReleaseNotes[],
    after: string,
    upTo: string,
): ReleaseNotes[] {
    return releases
        .filter(
            (release) =>
                isNewerVersion(release.version, after) &&
                compareVersions(release.version, upTo) <= 0 &&
                hasNotes(release),
        )
        .sort((a, b) => compareVersions(b.version, a.version));
}

/**
 * Notes to show on launch: every release after the last version the user saw,
 * up to the running one. Nothing when no version was recorded (a fresh install).
 */
export function whatsNewSince(
    changelog: ReleaseNotes[],
    lastSeenVersion: string | undefined,
    currentVersion: string,
): ReleaseNotes[] {
    if (!lastSeenVersion) {
        return [];
    }
    return releasesInRange(changelog, lastSeenVersion, currentVersion);
}

/** Pulls breaking changes out of `releases` (newest first) into one list. */
export function combineReleaseNotes(
    releases: ReleaseNotes[],
): CombinedReleaseNotes {
    const breaking: BreakingChange[] = [];
    const rest: ReleaseNotes[] = [];
    for (const release of releases) {
        for (const entry of release.sections.Breaking ?? []) {
            breaking.push({ entry, version: release.version });
        }
        const { Breaking: _breaking, ...sections } = release.sections;
        const remaining = { ...release, sections };
        if (hasNotes(remaining)) {
            rest.push(remaining);
        }
    }
    return { breaking, releases: rest };
}

/**
 * Whether an update from `installed` to `latest` breaks patches. Notes decide
 * when there are any; without notes, the version bump decides.
 */
export function updateHasBreakingChanges(
    notes: ReleaseNotes[],
    installed: string,
    latest: string,
): boolean {
    if (notes.length > 0) {
        return notes.some(
            (release) => (release.sections.Breaking?.length ?? 0) > 0,
        );
    }
    return isBreakingVersionBump(installed, latest);
}
