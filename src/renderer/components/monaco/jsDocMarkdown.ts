import type { JSDocTagInfo, SymbolDisplayPart, TextSpan } from 'typescript';

/** What a `{@link}` names: the text written in the tag, and its declaration
 * when the language service resolved one. */
export interface JsDocLinkRef {
    name: string;
    target?: { fileName: string; textSpan: TextSpan };
}

/** Returns the URI a link opens, or undefined to render it as plain code. */
export type JsDocLinkResolver = (ref: JsDocLinkRef) => string | undefined;

/** Link parts carry `target` on `linkName`; the worker serializes it as-is. */
type LinkPart = SymbolDisplayPart & { target?: JsDocLinkRef['target'] };

function codeSpan(text: string): string {
    const fence = text.includes('`') ? '``' : '`';
    return `${fence}${text}${fence}`;
}

function renderLink(
    name: string,
    label: string | undefined,
    target: JsDocLinkRef['target'],
    resolve: JsDocLinkResolver,
): string {
    const display = label
        ? label.replace(/[[\]]/g, '\\$&')
        : // Code span, so a name like `Poly<Signal>` is not taken as HTML.
          codeSpan(name);
    const uri = name ? resolve({ name, target }) : undefined;
    return uri ? `[${display}](${uri})` : display;
}

/**
 * Join display parts into Markdown, rendering each `{@link}` (TS splits it
 * into `link`, `linkName`, `linkText` parts) as a link the resolver chooses.
 */
export function docPartsToMarkdown(
    parts: readonly SymbolDisplayPart[] | undefined,
    resolve: JsDocLinkResolver,
): string {
    if (!parts) {
        return '';
    }
    let out = '';
    let link: { name: string; text: string; target?: JsDocLinkRef['target'] } | null =
        null;
    for (const part of parts as readonly LinkPart[]) {
        if (part.kind === 'link') {
            if (link) {
                // An unresolved link has no `linkName`; its text is the name.
                const text = link.text.trim();
                out += link.name
                    ? renderLink(link.name, text || undefined, link.target, resolve)
                    : renderLink(text, undefined, undefined, resolve);
                link = null;
            } else {
                link = { name: '', text: '' };
            }
        } else if (link && part.kind === 'linkName') {
            link.name += part.text;
            link.target = part.target;
        } else if (link && part.kind === 'linkText') {
            link.text += part.text;
        } else {
            out += part.text;
        }
    }
    return out;
}

function tagToMarkdown(tag: JSDocTagInfo, resolve: JsDocLinkResolver): string {
    const label = `*@${tag.name}*`;
    const parts = tag.text ?? [];
    if (tag.name === 'param' && parts[0]?.kind === 'parameterName') {
        const rest = docPartsToMarkdown(parts.slice(1), resolve).trim();
        return `${label} ${codeSpan(parts[0].text)}${rest ? ` — ${rest}` : ''}`;
    }
    const text = docPartsToMarkdown(parts, resolve).trim();
    if (tag.name === 'example' && text) {
        return `${label}\n${text.includes('```') ? text : `\`\`\`js\n${text}\n\`\`\``}`;
    }
    return text ? `${label} — ${text}` : label;
}

/** A symbol's documentation followed by its JSDoc tags, as Markdown. */
export function jsDocToMarkdown(
    documentation: readonly SymbolDisplayPart[] | undefined,
    tags: readonly JSDocTagInfo[] | undefined,
    resolve: JsDocLinkResolver,
): string {
    return [
        docPartsToMarkdown(documentation, resolve),
        ...(tags ?? []).map((tag) => tagToMarkdown(tag, resolve)),
    ]
        .filter(Boolean)
        .join('\n\n');
}
