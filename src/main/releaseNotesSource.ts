import { z } from 'zod';
import changelogMarkdown from '../../CHANGELOG.md?raw';
import {
    compareVersions,
    parseChangelog,
    parseReleaseBody,
    parseVersion,
    type ReleaseNotes,
} from '../shared/releaseNotes';

export interface PublishedRelease {
    notes: ReleaseNotes;
    url: string;
}

/** Bounds how long a slow or captive network keeps the release notes waiting. */
const FETCH_TIMEOUT_MS = 10_000;

const GitHubReleaseSchema = z.object({
    body: z.string().nullish(),
    draft: z.boolean(),
    html_url: z.string(),
    prerelease: z.boolean(),
    published_at: z.string().nullish(),
    tag_name: z.string(),
});

const GitHubReleaseListSchema = z.array(GitHubReleaseSchema);

function formatReleaseDate(isoDate: string | null | undefined): string | null {
    if (!isoDate) {
        return null;
    }
    const date = new Date(isoDate);
    if (Number.isNaN(date.getTime())) {
        return null;
    }
    return date.toLocaleDateString('en-US', {
        day: 'numeric',
        month: 'long',
        year: 'numeric',
    });
}

/**
 * Published releases of `repo` with plain `X.Y.Z` tags, newest first. Drafts
 * and prereleases are excluded. Returns null when GitHub answers with an error
 * or an unexpected shape; throws when the network is unreachable or the
 * request times out. Release bodies are untrusted network content.
 */
export async function fetchPublishedReleases(
    repo: string,
): Promise<PublishedRelease[] | null> {
    const response = await fetch(
        `https://api.github.com/repos/${repo}/releases?per_page=100`,
        {
            headers: { 'User-Agent': repo },
            signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
        },
    );
    if (!response.ok) {
        console.warn('[update] GitHub API returned', response.status);
        return null;
    }
    const parsed = GitHubReleaseListSchema.safeParse(await response.json());
    if (!parsed.success) {
        console.warn('[update] Unexpected GitHub API response shape');
        return null;
    }
    return parsed.data
        .filter(
            (release) =>
                !release.draft &&
                !release.prerelease &&
                parseVersion(release.tag_name),
        )
        .map((release) => ({
            notes: {
                date: formatReleaseDate(release.published_at),
                sections: parseReleaseBody(release.body ?? ''),
                version: release.tag_name.replace(/^v/, ''),
            },
            url: release.html_url,
        }))
        .sort((a, b) => compareVersions(b.notes.version, a.notes.version));
}

/** Release notes from the CHANGELOG.md bundled into the app at build time. */
export function bundledChangelog(): ReleaseNotes[] {
    return parseChangelog(changelogMarkdown);
}
