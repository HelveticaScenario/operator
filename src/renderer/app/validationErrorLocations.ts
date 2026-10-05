import type { ValidationError } from '@modular/core';
import type { SourceLocationInfo } from '../../shared/ipcTypes';

/**
 * Transform validation errors to use source line numbers instead of module IDs
 * for auto-generated modules (where the ID is meaningless to the user).
 *
 * Rust's format_module_location emits two shapes: `'myModule'` for explicit
 * user IDs (kept as-is) and `type(...)` for auto-generated IDs, which carries
 * only the module type. Auto-generated IDs are `type-N`, so an error can be
 * tied to a source line only when exactly one module of that type exists;
 * with several, the type hint is kept rather than guessing a line.
 */
export function transformErrorsWithSourceLocations(
    errors: ValidationError[],
    sourceLocationMap?: Record<string, SourceLocationInfo>,
): ValidationError[] {
    if (!sourceLocationMap) {
        return errors;
    }

    return errors.map((err) => {
        if (!err.location) {
            return err;
        }

        const explicitIdMatch = err.location.match(/^'([^']+)'$/);
        if (explicitIdMatch) {
            return err;
        }

        const autoMatch = err.location.match(/^(.*)\(\.\.\.\)$/);
        if (!autoMatch) {
            return err;
        }
        const moduleType = autoMatch[1];

        const candidates = Object.entries(sourceLocationMap).filter(
            ([moduleId, loc]) =>
                !loc.idIsExplicit &&
                moduleId.replace(/-\d+$/, '') === moduleType,
        );
        if (candidates.length !== 1) {
            return err;
        }

        return {
            ...err,
            location: `line ${candidates[0][1].line}`,
        };
    });
}
