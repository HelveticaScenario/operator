/**
 * Operator records, in a metadata block at the top of a patch file, the app
 * version under which the patch was last successfully evaluated. The block
 * looks like a JSDoc comment whose first tag is `@operator`, followed by
 * `@version <semver>`, e.g. a block holding `@operator` then `@version 0.0.101`.
 *
 * `@version` records the last version the patch successfully ran under, which
 * makes it a faithful proxy for the engine semantics the patch is known to be
 * compatible with: a migration that shipped after this version has not been
 * applied to the patch yet, so the patch may need it (see the migration
 * registry's selection).
 *
 * The block is written on save and stripped when the patch is loaded into the
 * editor, so it never appears to the user but always travels with the file on
 * disk (and through git, copy, zip, etc.). `@operator` identifies the block as
 * ours rather than a user's own JSDoc; the closing delimiter terminates it, and
 * the attribute list leaves room for more `@tag value` fields later.
 *
 * Functions here are pure string transforms with no Electron dependency, so the
 * main-process file handlers and the unit tests share the same logic.
 */

/** The metadata parsed out of a patch's leading Operator block. */
export interface PatchVersionStamp {
    /** The app version the patch was last successfully evaluated under, or
     *  `undefined` when the file carries no Operator block (never evaluated, or
     *  predating version stamping). */
    evaluatedVersion?: string;
}

/** File extensions that carry the version stamp (Operator patch files). */
const STAMPABLE_EXTENSIONS = ['.js', '.mjs'];

/**
 * Leading block comment, tolerating a UTF-8 BOM and surrounding whitespace.
 * Non-greedy up to the first closing delimiter so it captures exactly one
 * comment.
 */
const LEADING_BLOCK_COMMENT = /^\uFEFF?\s*\/\*[\s\S]*?\*\//;

/** The blank-line separator written between the block and the patch body. */
const TRAILING_SEPARATOR = /^[^\S\n]*\r?\n(?:[^\S\n]*\r?\n)?/;

/** Whether `filePath` is a patch file that carries the version stamp. */
export function isStampablePath(filePath: string): boolean {
    const lower = filePath.toLowerCase();
    return STAMPABLE_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

/**
 * Whether `block` is Operator's metadata block — it carries the `@operator`
 * tag as a standalone token, not merely as a substring inside other text.
 */
function isOperatorBlock(block: string): boolean {
    return /(^|[^\w@])@operator(?![\w-])/.test(block);
}

function buildBlock(evaluatedVersion: string): string {
    return [
        '/**',
        ' * @operator',
        ` * @version ${evaluatedVersion}`,
        ' */',
    ].join('\n');
}

/** Pull the value of a single `@tag` line out of an Operator block. */
function readTag(block: string, tag: string): string | undefined {
    const match = new RegExp(`@${tag}[^\\S\\n]+([^\\n*]+)`).exec(block);
    return match ? match[1].trim() : undefined;
}

/**
 * Read the version stamp from `content`. Returns the recorded
 * last-evaluated version, or `undefined` when there is no Operator block. A
 * leading comment that is not Operator's — a user's own header — is treated as
 * no stamp.
 */
export function parsePatchVersionStamp(content: string): PatchVersionStamp {
    const match = LEADING_BLOCK_COMMENT.exec(content);
    if (!match || !isOperatorBlock(match[0])) {
        return {};
    }
    return { evaluatedVersion: readTag(match[0], 'version') };
}

/**
 * Remove a leading Operator metadata block (and the blank line after it) when
 * present. A leading comment without the `@operator` tag — a user's own header
 * — is left untouched, as is a file that was never stamped. Idempotent.
 */
export function stripPatchVersionStamp(content: string): string {
    const match = LEADING_BLOCK_COMMENT.exec(content);
    if (!match || !isOperatorBlock(match[0])) {
        return content;
    }
    return content.slice(match[0].length).replace(TRAILING_SEPARATOR, '');
}

/**
 * Return `content` with a fresh metadata block recording `evaluatedVersion` at
 * the top. Any existing block is replaced first, so re-stamping is idempotent
 * and always reflects the latest successful-evaluation version.
 */
export function stampPatchVersionSource(
    content: string,
    evaluatedVersion: string,
): string {
    const body = stripPatchVersionStamp(content);
    const block = buildBlock(evaluatedVersion);
    return body.length === 0 ? `${block}\n` : `${block}\n\n${body}`;
}
