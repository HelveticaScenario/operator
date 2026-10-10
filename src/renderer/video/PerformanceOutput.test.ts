// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PERFORMANCE_WINDOW_NAME } from '../../shared/video/performanceWindowName';
import { PerformanceOutput } from './PerformanceOutput';

/** A stand-in for the window `window.open` returns, with a real document to build in. */
function fakeChild() {
    const document = window.document.implementation.createHTMLDocument('');
    const draw = vi.fn();
    const context = { drawImage: draw };
    const createElement = document.createElement.bind(document) as (
        tag: string,
    ) => HTMLElement;
    document.createElement = ((tag: string) => {
        const element = createElement(tag);
        if (tag === 'canvas') {
            (element as HTMLCanvasElement).getContext = (() =>
                context) as never;
        }
        return element;
    }) as typeof document.createElement;
    const listeners = new Map<string, () => void>();
    const child = {
        addEventListener: (type: string, listener: () => void) =>
            listeners.set(type, listener),
        close: vi.fn(() => {
            child.closed = true;
        }),
        closed: false,
        devicePixelRatio: 2,
        document,
        focus: vi.fn(),
        innerHeight: 400,
        innerWidth: 700,
        removeEventListener: (type: string) => listeners.delete(type),
    };
    return { child, draw, listeners };
}

describe('PerformanceOutput', () => {
    let open: ReturnType<typeof vi.fn>;
    let made: ReturnType<typeof fakeChild>;

    beforeEach(() => {
        made = fakeChild();
        open = vi.fn(() => made.child);
        vi.stubGlobal('open', open);
        vi.spyOn(window, 'focus').mockImplementation(() => undefined);
    });

    afterEach(() => {
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
    });

    it('opens a window under the performance name and keeps the editor focused', () => {
        const output = new PerformanceOutput();
        output.open();
        expect(open).toHaveBeenCalledWith(
            'about:blank',
            PERFORMANCE_WINDOW_NAME,
            expect.stringContaining('width='),
        );
        expect(output.isOpen).toBe(true);
        expect(window.focus).toHaveBeenCalled();
        expect(made.child.document.querySelector('canvas')).not.toBeNull();
    });

    it('reports the size of the window in device pixels', () => {
        const output = new PerformanceOutput();
        expect(output.size).toBeNull();
        output.open();
        expect(output.size).toEqual({ height: 800, width: 1400 });
    });

    it('has no size while the window has no area', () => {
        const output = new PerformanceOutput();
        output.open();
        made.child.innerWidth = 0;
        expect(output.size).toBeNull();
    });

    it('copies a frame into the window, at the size of the frame', () => {
        const output = new PerformanceOutput();
        output.open();
        const source = document.createElement('canvas');
        source.width = 1400;
        source.height = 800;
        output.show(source);
        const target = made.child.document.querySelector('canvas')!;
        expect(target.width).toBe(1400);
        expect(target.height).toBe(800);
        expect(made.draw).toHaveBeenCalledWith(source, 0, 0);
    });

    it('notifies when the window opens, resizes and closes', () => {
        const output = new PerformanceOutput();
        const listener = vi.fn();
        output.subscribe(listener);
        output.open();
        expect(listener).toHaveBeenCalledTimes(1);
        made.listeners.get('resize')!();
        expect(listener).toHaveBeenCalledTimes(2);
        output.close();
        expect(listener).toHaveBeenCalledTimes(3);
        expect(made.child.close).toHaveBeenCalled();
        expect(output.isOpen).toBe(false);
    });

    it('notices a window the user closed', () => {
        const output = new PerformanceOutput();
        const listener = vi.fn();
        output.open();
        output.subscribe(listener);
        made.child.closed = true;
        made.listeners.get('pagehide')!();
        expect(output.isOpen).toBe(false);
        expect(listener).toHaveBeenCalledTimes(1);
    });

    it('stops copying once the window is gone', () => {
        const output = new PerformanceOutput();
        output.open();
        made.child.closed = true;
        output.show(document.createElement('canvas'));
        expect(made.draw).not.toHaveBeenCalled();
        expect(output.isOpen).toBe(false);
    });

    it('focuses an open window rather than opening another', () => {
        const output = new PerformanceOutput();
        output.open();
        output.open();
        expect(open).toHaveBeenCalledTimes(1);
        expect(made.child.focus).toHaveBeenCalled();
    });

    it('toggles', () => {
        const output = new PerformanceOutput();
        output.toggle();
        expect(output.isOpen).toBe(true);
        output.toggle();
        expect(output.isOpen).toBe(false);
    });
});
