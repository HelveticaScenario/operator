import type { AppTheme } from '../themes/types';

/** How a token is drawn: its color and whether it is bold or italic. */
export interface TokenStyle {
    color: string;
    bold: boolean;
    italic: boolean;
}

/**
 * The TextMate scopes, most preferred first, that a theme colors each of
 * Monaco's JavaScript token types by. A `$` function is a DSL module call.
 */
const SCOPES: Record<string, readonly string[]> = {
    comment: ['comment'],
    'comment.doc': ['comment.block.documentation', 'comment'],
    function: ['entity.name.function', 'support.function'],
    keyword: ['keyword', 'storage.type', 'keyword.control'],
    number: ['constant.numeric', 'constant'],
    regexp: ['string.regexp', 'string'],
    string: ['string'],
    'string.escape': ['constant.character.escape', 'string'],
    'string.invalid': ['invalid', 'string'],
    'type.identifier': ['entity.name.type', 'support.type', 'storage.type'],
};

/** Whether the TextMate selector `rule` applies to `scope`: equal, or a parent of it. */
const covers = (rule: string, scope: string) =>
    scope === rule || scope.startsWith(`${rule}.`);

/**
 * The style a theme gives a TextMate `scope`: the most specific rule that
 * covers it, or null when none does.
 */
function scopeStyle(theme: AppTheme, scope: string): TokenStyle | null {
    let best: { length: number; style: TokenStyle } | null = null;
    for (const { scope: selectors, settings } of theme.raw.tokenColors) {
        if (settings.foreground === undefined) continue;
        const list = Array.isArray(selectors)
            ? selectors
            : (selectors ?? '').split(',');
        for (const raw of list) {
            const rule = raw.trim();
            if (rule === '' || rule.includes(' ') || !covers(rule, scope)) {
                continue;
            }
            if (best !== null && rule.length <= best.length) continue;
            const fontStyle = settings.fontStyle ?? '';
            best = {
                length: rule.length,
                style: {
                    bold: fontStyle.includes('bold'),
                    color: settings.foreground,
                    italic: fontStyle.includes('italic'),
                },
            };
        }
    }
    return best?.style ?? null;
}

/**
 * Resolves Monaco token types (`keyword.js`, `identifier.js`, ...) to the
 * colors a theme gives them. Results are cached per type.
 */
export class TokenColors {
    private readonly cache = new Map<string, TokenStyle>();
    private readonly plain: TokenStyle;

    constructor(private readonly theme: AppTheme) {
        this.plain = {
            bold: false,
            color: theme.colors.editorForeground,
            italic: false,
        };
    }

    /** The style of a token of `type` whose text is `text`. */
    style(type: string, text: string): TokenStyle {
        const base = type.replace(/\.js$/, '');
        const key =
            base === 'identifier' && text.startsWith('$') ? 'function' : base;
        let style = this.cache.get(key);
        if (style === undefined) {
            style = this.resolve(key);
            this.cache.set(key, style);
        }
        return style;
    }

    private resolve(type: string): TokenStyle {
        // A dotted type such as `delimiter.bracket` falls back to its parent.
        let t = type;
        for (;;) {
            for (const scope of SCOPES[t] ?? []) {
                const style = scopeStyle(this.theme, scope);
                if (style !== null) return style;
            }
            const dot = t.lastIndexOf('.');
            if (dot < 0) return this.plain;
            t = t.slice(0, dot);
        }
    }
}
