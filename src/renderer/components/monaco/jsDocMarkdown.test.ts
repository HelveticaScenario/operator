/**
 * Renders JSDoc from the real TypeScript language service, so the tests track
 * the display-part shapes TS gives `{@link}` tags.
 */
import ts from 'typescript';
import { describe, expect, test } from 'vitest';
import type { JsDocLinkRef } from './jsDocMarkdown';
import { jsDocToMarkdown } from './jsDocMarkdown';

const LIB = `
interface Options { gain?: number }
/**
 * Send it out with {@link Options}, see {@link Thing.go | the go method}.
 * @param options Output options ({@link Options})
 * @param level A {@link Poly<number>} level
 * @see {@link missing}
 * @example out({ gain: 2 })
 */
declare function out(options?: Options, level?: number): void;
declare namespace Thing { function go(): void }
type Poly<T> = T | T[];
`;

/** Quick info for the function declared by `declare function <name>`. */
function quickInfo(source: string, name: string) {
    const files: Record<string, string> = { '/lib.ts': source };
    const host: ts.LanguageServiceHost = {
        fileExists: (f) => f in files || ts.sys.fileExists(f),
        getCompilationSettings: () => ({ lib: ['lib.es2020.d.ts'], noLib: false }),
        getCurrentDirectory: () => '/',
        getDefaultLibFileName: (o) => ts.getDefaultLibFilePath(o),
        getScriptFileNames: () => Object.keys(files),
        getScriptSnapshot: (f) => {
            const text = files[f] ?? ts.sys.readFile(f);
            return text === undefined ? undefined : ts.ScriptSnapshot.fromString(text);
        },
        getScriptVersion: () => '1',
        readFile: (f) => files[f] ?? ts.sys.readFile(f),
    };
    const info = ts
        .createLanguageService(host)
        .getQuickInfoAtPosition(
            '/lib.ts',
            source.indexOf(`declare function ${name}`) + 'declare function '.length,
        );
    if (!info) {
        throw new Error(`no quick info for ${name}`);
    }
    return info;
}

describe('jsDocToMarkdown', () => {
    const info = quickInfo(LIB, 'out');

    test('renders each resolved link through the resolver', () => {
        const refs: JsDocLinkRef[] = [];
        const markdown = jsDocToMarkdown(info.documentation, info.tags, (ref) => {
            refs.push(ref);
            return ref.name === 'missing' ? undefined : `command:open?${ref.name}`;
        });

        expect(markdown).toBe(
            [
                'Send it out with [`Options`](command:open?Options), see [the go method](command:open?Thing.go).',
                '*@param* `options` — Output options ([`Options`](command:open?Options))',
                '*@param* `level` — A [`Poly<number>`](command:open?Poly<number>) level',
                '*@see* — `missing`',
                '*@example*\n```js\nout({ gain: 2 })\n```',
            ].join('\n\n'),
        );
        const options = refs.find((r) => r.name === 'Options');
        expect(options?.target?.fileName).toBe('/lib.ts');
        expect(
            LIB.slice(
                options!.target!.textSpan.start,
                options!.target!.textSpan.start + options!.target!.textSpan.length,
            ),
        ).toBe('interface Options { gain?: number }');
        expect(refs.find((r) => r.name === 'missing')?.target).toBeUndefined();
    });

    test('renders links as code when nothing resolves them', () => {
        expect(jsDocToMarkdown(info.documentation, [], () => undefined)).toBe(
            'Send it out with `Options`, see the go method.',
        );
    });

    test('keeps an example that already has a fence as written', () => {
        const fenced = quickInfo(
            '/**\n * @example\n * ```js\n * a()\n * ```\n */\ndeclare function a(): void;',
            'a',
        );
        expect(jsDocToMarkdown(fenced.documentation, fenced.tags, () => undefined)).toBe(
            '*@example*\n```js\na()\n```',
        );
    });
});
