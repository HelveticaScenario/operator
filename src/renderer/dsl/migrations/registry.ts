import { compareVersion } from '../../../shared/compareVersion';
import { meta as adsrDefaultsMigration } from '../migrateAdsrDefaults';
import { meta as chebyBlockDcMigration } from '../migrateChebyBlockDC';
import { meta as cycleCallsMigration } from '../migrateCycleCalls';
import { meta as wavetableArgsMigration } from '../migrateWavetableArgs';
import type { MigrationMeta } from './types';
import { NEXT_VERSION } from './types';

export type { MigrationMeta, MigrationRunResult } from './types';
export { NEXT_VERSION } from './types';

/** Every known patch migration, in application order. New migrations register
 *  by adding their `meta` here; the order field is the source of truth so the
 *  array literal order does not matter. */
export const MIGRATIONS: MigrationMeta[] = [
    cycleCallsMigration,
    wavetableArgsMigration,
    chebyBlockDcMigration,
    adsrDefaultsMigration,
].sort((a, b) => a.order - b.order);

/**
 * The migrations a patch still needs, in application order, given the app
 * version it was last successfully evaluated under. A migration that shipped
 * after that version has not been applied to the patch's semantics yet:
 *
 * - An unreleased ({@link NEXT_VERSION}) migration is newer than any released
 *   version, so no patch conforms to it yet — always offered.
 * - A patch never evaluated (no stamp) is treated as eligible for any migration.
 * - Otherwise the patch needs the migration iff it last evaluated before the
 *   migration shipped.
 */
export function migrationsNeededFor(
    evaluatedVersion: string | undefined,
): MigrationMeta[] {
    return MIGRATIONS.filter((migration) => {
        if (migration.sinceVersion === NEXT_VERSION) return true;
        if (evaluatedVersion === undefined) return true;
        return compareVersion(evaluatedVersion, migration.sinceVersion) < 0;
    });
}
