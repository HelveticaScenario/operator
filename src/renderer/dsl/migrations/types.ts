import type { MigrationModalSummary } from '../../components/MigrationDiffModal';

/** Sentinel `sinceVersion` for a migration that has not shipped in a release
 *  yet. `release.sh` rewrites it to the new version when cutting a release, so
 *  the developer never has to guess the release number while authoring. Until
 *  then it sorts as newer than any stamped version, so the migration is offered
 *  on every patch built against the development tree. */
export const NEXT_VERSION = 'next';

/** The normalized outcome of running a migration, shared by the registry-driven
 *  runner and the diff modal regardless of which migration produced it. */
export interface MigrationRunResult {
    /** The source after the migration (equal to the input when nothing or only
     *  flagged-but-unrewritten constructs were found). */
    migrated: string;
    /** Whether the migration actually rewrote the source. */
    changed: boolean;
    /** Counts and skipped items for the diff modal. */
    summary: MigrationModalSummary;
}

/** A migration's identity, the release it shipped in, and how to run it. One
 *  `meta` is co-located with each migration's implementation; the registry
 *  collects them into an ordered list. */
export interface MigrationMeta {
    /** Unique, stable identifier for this migration. The registry requires ids
     *  to be distinct; it is not written into patches. */
    id: string;
    /** The release version the migration shipped in, or {@link NEXT_VERSION}
     *  while unreleased. A patch last written before this version may need it. */
    sinceVersion: string;
    /** Application order when several migrations apply to one patch. Monotonic
     *  and independent of `sinceVersion`, which disambiguates migrations that
     *  ship in the same release. */
    order: number;
    /** Heading shown in the diff modal. */
    title: string;
    /** Label prefixing the modal's list of constructs left for manual review. */
    skippedLabel?: string;
    /** Run the migration against `source`, normalized for the runner/modal. */
    run(source: string): MigrationRunResult;
}
