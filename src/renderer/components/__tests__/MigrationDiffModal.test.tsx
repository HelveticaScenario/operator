import { describe, expect, test, vi } from 'vitest';
import * as React from 'react';

// The component depends on a CSS file, a Monaco DiffEditor, and a ThemeContext
// hook backed by Electron IPC. None of that is available in the node test
// environment, so each dependency gets a hand-rolled stub before importing the
// component under test.
//
// The component is invoked as a plain function (not through ReactDOM) so the
// returned element tree can be inspected directly. React's hook dispatcher is
// only wired up during a real render, so useRef / useEffect / useCallback are
// stubbed to no-op equivalents that let the component body execute without
// requiring a renderer. The DiffEditor stub is the same module-level function
// reference exposed to the component via vi.mock, so tests find it in the
// element tree by reference identity and read its props in place.

vi.mock('react', async (importOriginal) => {
    const actual =
        await importOriginal<typeof import('react')>();
    return {
        ...actual,
        useRef: (initial?: unknown) => ({ current: initial }),
        useEffect: () => undefined,
        useCallback: <T,>(cb: T) => cb,
    };
});

vi.mock('../MigrationDiffModal.css', () => ({}));

// Sentinel function reference for the DiffEditor stub. The component is
// invoked directly (no ReactDOM), so the stub is never called — it appears in
// the returned element tree as `{ type: DIFF_EDITOR_STUB, props }` and tests
// inspect those props in place of mock-call recording. vi.hoisted lets the
// vi.mock factory reference this without TDZ errors from mock hoisting.
const { DIFF_EDITOR_STUB } = vi.hoisted(() => ({
    DIFF_EDITOR_STUB: (_props: unknown) => null,
}));

vi.mock('@monaco-editor/react', () => ({
    DiffEditor: DIFF_EDITOR_STUB,
}));

vi.mock('../../themes/ThemeContext', () => ({
    useTheme: () => ({
        theme: { id: 'modular-dark', type: 'dark', colors: {} },
        font: 'Fira Code',
        fontLigatures: true,
        fontSize: 17,
    }),
}));

vi.mock('../monaco/theme', () => ({
    applyMonacoTheme: () => undefined,
}));

import {
    MigrationDiffModal,
    type MigrationModalSummary,
} from '../MigrationDiffModal';

// ---------------------------------------------------------------------------
// Element-tree walking helpers
//
// The component is invoked as a plain function so we can inspect the returned
// React element tree without a DOM. The tree is a tagged union of ReactElement
// nodes, primitives, and arrays; walkElements recursively yields every
// ReactElement so individual tests can filter for buttons or stubbed
// subcomponents by predicate.
// ---------------------------------------------------------------------------

type AnyEl = React.ReactElement<Record<string, unknown>>;

function isElement(x: unknown): x is AnyEl {
    return (
        typeof x === 'object' &&
        x !== null &&
        'type' in (x as Record<string, unknown>) &&
        'props' in (x as Record<string, unknown>)
    );
}

function* walkElements(node: unknown): Generator<AnyEl> {
    if (node == null || typeof node === 'boolean') return;
    if (Array.isArray(node)) {
        for (const child of node) yield* walkElements(child);
        return;
    }
    if (isElement(node)) {
        yield node;
        const children = (node.props as { children?: unknown }).children;
        if (children !== undefined) yield* walkElements(children);
    }
}

function renderModal(props: {
    isOpen?: boolean;
    original?: string;
    migrated?: string;
    summary?: Partial<MigrationModalSummary>;
    onApply?: () => void;
    onCancel?: () => void;
}): AnyEl | null {
    const summary: MigrationModalSummary = {
        callsChanged: 0,
        assignmentsChanged: 0,
        commentsChanged: 0,
        skippedVariables: [],
        ...props.summary,
    };
    return MigrationDiffModal({
        isOpen: props.isOpen ?? true,
        original: props.original ?? '',
        migrated: props.migrated ?? '',
        summary,
        onApply: props.onApply ?? (() => undefined),
        onCancel: props.onCancel ?? (() => undefined),
    }) as AnyEl | null;
}

function findButtonByText(root: AnyEl, text: string): AnyEl {
    for (const el of walkElements(root)) {
        if (el.type !== 'button') continue;
        const children = (el.props as { children?: unknown }).children;
        const flat = Array.isArray(children) ? children.join('') : children;
        if (typeof flat === 'string' && flat.trim() === text) return el;
    }
    throw new Error(`no <button> with text "${text}" in tree`);
}

function findDiffEditorStub(root: AnyEl): AnyEl | null {
    for (const el of walkElements(root)) {
        if (el.type === DIFF_EDITOR_STUB) return el;
    }
    return null;
}

describe('MigrationDiffModal', () => {
    test('returns null when closed', () => {
        const tree = renderModal({ isOpen: false });
        expect(tree).toBeNull();
    });

    test('forwards original/migrated text to the diff editor', () => {
        const tree = renderModal({
            original: 'const a = 1;',
            migrated: 'const a = 2;',
            summary: { callsChanged: 1 },
        });
        expect(tree).not.toBeNull();

        const stub = findDiffEditorStub(tree!);
        expect(stub).not.toBeNull();
        const props = stub!.props as {
            original: string;
            modified: string;
            language: string;
            options: { readOnly: boolean; renderSideBySide: boolean };
        };
        expect(props.original).toBe('const a = 1;');
        expect(props.modified).toBe('const a = 2;');
        expect(props.language).toBe('javascript');
        expect(props.options.readOnly).toBe(true);
        expect(props.options.renderSideBySide).toBe(true);
    });

    test('shows empty-state message and no diff editor when totalChanges === 0', () => {
        const tree = renderModal({
            original: 'x',
            migrated: 'x',
            summary: {
                callsChanged: 0,
                assignmentsChanged: 0,
                commentsChanged: 0,
            },
        });
        expect(findDiffEditorStub(tree!)).toBeNull();

        const texts: string[] = [];
        for (const el of walkElements(tree!)) {
            const children = (el.props as { children?: unknown }).children;
            if (typeof children === 'string') texts.push(children);
        }
        expect(
            texts.some((t) =>
                t.includes('Buffer already migrated'),
            ),
        ).toBe(true);
    });

    test('Apply button is disabled when totalChanges === 0', () => {
        const tree = renderModal({
            summary: {
                callsChanged: 0,
                assignmentsChanged: 0,
                commentsChanged: 0,
            },
        });
        const apply = findButtonByText(tree!, 'Apply');
        expect((apply.props as { disabled?: boolean }).disabled).toBe(true);
    });

    test('Apply button is disabled when error is set', () => {
        const tree = renderModal({
            summary: {
                callsChanged: 3,
                assignmentsChanged: 1,
                commentsChanged: 0,
                error: 'parse failure',
            },
        });
        const apply = findButtonByText(tree!, 'Apply');
        expect((apply.props as { disabled?: boolean }).disabled).toBe(true);

        // The error message should also render in the summary block.
        const texts: string[] = [];
        for (const el of walkElements(tree!)) {
            const children = (el.props as { children?: unknown }).children;
            const flat = Array.isArray(children)
                ? children.filter((c) => typeof c === 'string').join('')
                : children;
            if (typeof flat === 'string') texts.push(flat);
        }
        expect(texts.some((t) => t.includes('parse failure'))).toBe(true);
    });

    test('Apply button is enabled when totalChanges > 0 and no error', () => {
        const tree = renderModal({
            summary: {
                callsChanged: 1,
                assignmentsChanged: 0,
                commentsChanged: 0,
            },
        });
        const apply = findButtonByText(tree!, 'Apply');
        // `disabled={!canApply}` resolves to `false`, not `undefined`.
        expect((apply.props as { disabled?: boolean }).disabled).toBe(false);
    });

    test('Apply totalChanges sums calls + assignments + comments', () => {
        // callsChanged=0 but assignmentsChanged>0 should still enable Apply,
        // i.e. the disabled check is on the *sum*, not on calls alone.
        const tree = renderModal({
            summary: {
                callsChanged: 0,
                assignmentsChanged: 2,
                commentsChanged: 0,
            },
        });
        const apply = findButtonByText(tree!, 'Apply');
        expect((apply.props as { disabled?: boolean }).disabled).toBe(false);
    });

    test('Cancel button onClick invokes onCancel', () => {
        const onCancel = vi.fn();
        const onApply = vi.fn();
        const tree = renderModal({
            summary: { callsChanged: 1 },
            onCancel,
            onApply,
        });
        const cancel = findButtonByText(tree!, 'Cancel');
        const handler = (cancel.props as { onClick: () => void }).onClick;
        handler();
        expect(onCancel).toHaveBeenCalledTimes(1);
        expect(onApply).not.toHaveBeenCalled();
    });

    test('Apply button onClick invokes onApply', () => {
        const onCancel = vi.fn();
        const onApply = vi.fn();
        const tree = renderModal({
            summary: { callsChanged: 2 },
            onCancel,
            onApply,
        });
        const apply = findButtonByText(tree!, 'Apply');
        const handler = (apply.props as { onClick: () => void }).onClick;
        handler();
        expect(onApply).toHaveBeenCalledTimes(1);
        expect(onCancel).not.toHaveBeenCalled();
    });

    test('renders skipped-variable warning when list non-empty', () => {
        const tree = renderModal({
            summary: {
                callsChanged: 1,
                skippedVariables: ['foo', 'bar'],
            },
        });
        const texts: string[] = [];
        for (const el of walkElements(tree!)) {
            const children = (el.props as { children?: unknown }).children;
            const flat = Array.isArray(children)
                ? children.filter((c) => typeof c === 'string').join('')
                : children;
            if (typeof flat === 'string') texts.push(flat);
        }
        const joined = texts.join(' | ');
        expect(joined).toContain('Skipped variables');
        expect(joined).toContain('foo, bar');
    });
});
