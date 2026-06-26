/**
 * Operator records the app version that last wrote a patch in a metadata block
 * at the top of the file. The block looks like a JSDoc comment whose first tag
 * is `@operator`, followed by `@version <semver>`, e.g. a block holding
 * `@operator` then `@version 0.0.101`.
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

function buildBlock(version: string): string {
    return ['/**', ' * @operator', ` * @version ${version}`, ' */'].join('\n');
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
 * Return `content` with a fresh metadata block for `version` at the top. Any
 * existing block is replaced first, so re-stamping is idempotent and always
 * reflects the version that wrote the file last.
 */
export function stampPatchVersionSource(
    content: string,
    version: string,
): string {
    const body = stripPatchVersionStamp(content);
    const block = buildBlock(version);
    return body.length === 0 ? `${block}\n` : `${block}\n\n${body}`;
}
