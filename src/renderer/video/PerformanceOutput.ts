import { PERFORMANCE_WINDOW_NAME } from '../../shared/video/performanceWindowName';

/** The pixel size of the performance window's picture. */
export interface OutputSize {
    height: number;
    width: number;
}

/**
 * The performance window: a second window the editor opens, holding a canvas
 * that each rendered frame is copied into. The picture is drawn once, by the
 * editor window's renderer, so the two windows always show the same frame.
 */
export class PerformanceOutput {
    private child: Window | null = null;
    private canvas: HTMLCanvasElement | null = null;
    private context: CanvasRenderingContext2D | null = null;
    private readonly listeners = new Set<() => void>();

    get isOpen(): boolean {
        return this.child !== null && !this.child.closed;
    }

    /** The window's content size in device pixels, or null while it is closed. */
    get size(): OutputSize | null {
        const { child } = this;
        if (child === null || child.closed) return null;
        const width = Math.round(child.innerWidth * child.devicePixelRatio);
        const height = Math.round(child.innerHeight * child.devicePixelRatio);
        return width > 0 && height > 0 ? { height, width } : null;
    }

    /** Calls `listener` when the window opens, closes or resizes. */
    subscribe(listener: () => void): () => void {
        this.listeners.add(listener);
        return () => {
            this.listeners.delete(listener);
        };
    }

    toggle(): void {
        if (this.isOpen) this.close();
        else this.open();
    }

    open(): void {
        if (this.isOpen) {
            this.child?.focus();
            return;
        }
        const child = window.open(
            'about:blank',
            PERFORMANCE_WINDOW_NAME,
            'width=1280,height=720',
        );
        if (child === null) return;
        const { document } = child;
        document.title = 'Operator Performance';
        document.body.style.cssText =
            'margin:0;background:#000;overflow:hidden';
        const canvas = document.createElement('canvas');
        canvas.style.cssText =
            'display:block;width:100vw;height:100vh;object-fit:contain';
        document.body.appendChild(canvas);
        this.child = child;
        this.canvas = canvas;
        this.context = canvas.getContext('2d');
        child.addEventListener('resize', this.notify);
        child.addEventListener('pagehide', this.release);
        window.addEventListener('pagehide', this.close);
        // The editor keeps the keyboard.
        window.focus();
        this.notify();
    }

    close = (): void => {
        const { child } = this;
        this.release();
        child?.close();
    };

    /** Copies a rendered frame into the window. */
    show(source: HTMLCanvasElement): void {
        if (this.child?.closed) this.release();
        const { canvas, context } = this;
        if (canvas === null || context === null) return;
        if (canvas.width !== source.width || canvas.height !== source.height) {
            canvas.width = source.width;
            canvas.height = source.height;
        }
        context.drawImage(source, 0, 0);
    }

    private release = (): void => {
        if (this.child === null) return;
        this.child.removeEventListener('resize', this.notify);
        this.child.removeEventListener('pagehide', this.release);
        window.removeEventListener('pagehide', this.close);
        this.child = null;
        this.canvas = null;
        this.context = null;
        this.notify();
    };

    private notify = (): void => {
        for (const listener of this.listeners) listener();
    };
}

export const performanceOutput = new PerformanceOutput();
