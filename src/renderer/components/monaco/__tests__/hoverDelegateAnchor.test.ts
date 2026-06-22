import { beforeEach, describe, expect, it, vi } from 'vitest';

// monaco's hover-delegate factory is a process-global singleton backed by a
// module-level Lazy that resolves exactly once. Reset the module registry
// between cases so each test gets a fresh, unresolved Lazy.
const FACTORY_PATH =
    'monaco-editor/esm/vs/base/browser/ui/hover/hoverDelegateFactory.js';

type HoverFactoryModule = {
    setHoverDelegateFactory: (
        factory: (
            placement: 'mouse' | 'element',
            enableInstantHover: boolean,
        ) => unknown,
    ) => void;
    getDefaultHoverDelegate: (placement: 'mouse' | 'element') => unknown;
};

const liveDelegate = { delay: -1, dispose() {}, showHover: () => undefined };

// A fake monaco whose editor.create mimics a real StandaloneEditor constructor:
// it overwrites monaco's global hover-delegate factory with its own service.
// Here that service is "immortal" (never disposed), standing in for the anchor.
function makeFakeMonaco(
    setFactory: HoverFactoryModule['setHoverDelegateFactory'],
) {
    let created = 0;
    return {
        editor: {
            create: () => {
                created++;
                setFactory(() => liveDelegate);
                return {} as never;
            },
        },
        get createCount() {
            return created;
        },
    };
}

describe('installHoverDelegateAnchor (monaco-editor#4612 workaround)', () => {
    beforeEach(() => {
        vi.resetModules();
        // installHoverDelegateAnchor creates a detached anchor container via
        // document.createElement; provide a minimal stub (no DOM env here).
        (globalThis as { document?: unknown }).document = {
            createElement: () => ({}),
        };
    });

    it('reproduces the bug: without an anchor, a disposed factory throws on first resolve', async () => {
        const { setHoverDelegateFactory, getDefaultHoverDelegate } =
            (await import(FACTORY_PATH)) as HoverFactoryModule;

        // Main editor sets a live factory, then a transient editor overwrites
        // it and is disposed, so its closure now throws like a disposed
        // instantiation service would.
        setHoverDelegateFactory(() => liveDelegate);
        setHoverDelegateFactory(() => {
            throw new Error('InstantiationService has been disposed');
        });

        expect(() => getDefaultHoverDelegate('element')).toThrow(
            'InstantiationService has been disposed',
        );
    });

    it('pins the default hover delegate to an immortal service, surviving a later disposed editor', async () => {
        const factory = (await import(FACTORY_PATH)) as HoverFactoryModule;
        const { installHoverDelegateAnchor } =
            await import('../hoverDelegateAnchor');
        const fakeMonaco = makeFakeMonaco(factory.setHoverDelegateFactory);

        // Anchor created (sets the immortal factory) and Lazy pinned to it.
        installHoverDelegateAnchor(fakeMonaco as never);
        // A transient editor overwrites the factory and is disposed.
        factory.setHoverDelegateFactory(() => {
            throw new Error('InstantiationService has been disposed');
        });

        expect(() => factory.getDefaultHoverDelegate('element')).not.toThrow();
        expect(factory.getDefaultHoverDelegate('element')).toBe(liveDelegate);
    });

    it('is idempotent: repeated installs do not create another anchor', async () => {
        const factory = (await import(FACTORY_PATH)) as HoverFactoryModule;
        const { installHoverDelegateAnchor } =
            await import('../hoverDelegateAnchor');
        const fakeMonaco = makeFakeMonaco(factory.setHoverDelegateFactory);

        installHoverDelegateAnchor(fakeMonaco as never);
        installHoverDelegateAnchor(fakeMonaco as never);

        expect(fakeMonaco.createCount).toBe(1);
    });
});
