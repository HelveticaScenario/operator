import { PERFORMANCE_WINDOW_NAME } from '../../shared/video/performanceWindowName';
import type { PerformanceCode } from '../performance/PerformanceCode';

/** The pixel size of the performance window's picture. */
export interface OutputSize {
    height: number;
    width: number;
}

/** How long the code takes to fade fully in or out, in milliseconds. */
const CODE_FADE_MS = 400;

/**
 * The performance window: a second window the editor opens, holding a canvas
 * that shows the patch's video with the code laid over it. The picture is
 * drawn once, by the editor window's renderer, and copied here; each display
 * frame the latest picture and the code are composited into the window, and
 * the result is handed to anything showing a preview of it.
 */
export class PerformanceOutput {
    private child: Window | null = null;
    private canvas: HTMLCanvasElement | null = null;
    private context: CanvasRenderingContext2D | null = null;
    /** The latest video frame, kept so the code can be redrawn over it. */
    private readonly video = document.createElement('canvas');
    private readonly videoContext = this.video.getContext('2d');
    private videoFresh = false;
    private code: PerformanceCode | null = null;
    /** Opacity the code is fading toward, and its opacity now. */
    private codeTarget = 1;
    private codeAlpha = 1;
    /** Set when something outside the frame loop changes what is shown. */
    private stale = true;
    private frameHandle = 0;
    private lastMs = -1;
    private readonly listeners = new Set<() => void>();
    private readonly compositeListeners = new Set<
        (canvas: HTMLCanvasElement) => void
    >();
    private scale = 1;

    constructor() {
        this.video.width = 0;
        this.video.height = 0;
    }

    get isOpen(): boolean {
        return this.child !== null && !this.child.closed;
    }

    /**
     * The size the picture is drawn at: the window's content size in device
     * pixels times the resolution scale, or null while it is closed. The
     * window shows the picture stretched to fit.
     */
    get size(): OutputSize | null {
        const { child } = this;
        if (child === null || child.closed) return null;
        const scale = child.devicePixelRatio * this.scale;
        const width = Math.round(child.innerWidth * scale);
        const height = Math.round(child.innerHeight * scale);
        return width > 0 && height > 0 ? { height, width } : null;
    }

    /** Draws the picture at `scale` of the window's pixels, from 0.1 to 1. */
    setScale(scale: number): void {
        if (scale === this.scale) return;
        this.scale = scale;
        this.notify();
    }

    /** The code to lay over the picture, or null for the picture alone. */
    setCode(code: PerformanceCode | null): void {
        this.code = code;
        this.stale = true;
    }

    /**
     * Fades the code toward `opacity`, from 0 (hidden) to 1. A window that
     * opens starts at that opacity rather than fading to it.
     */
    setCodeOpacity(opacity: number): void {
        this.codeTarget = opacity;
        this.stale = true;
    }

    /** Calls `listener` when the window opens, closes or resizes. */
    subscribe(listener: () => void): () => void {
        this.listeners.add(listener);
        return () => {
            this.listeners.delete(listener);
        };
    }

    /** Calls `listener` with the window's canvas each time a new picture is in it. */
    onComposite(listener: (canvas: HTMLCanvasElement) => void): () => void {
        this.compositeListeners.add(listener);
        return () => {
            this.compositeListeners.delete(listener);
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
        this.stale = true;
        this.lastMs = -1;
        child.addEventListener('resize', this.notify);
        child.addEventListener('pagehide', this.release);
        window.addEventListener('pagehide', this.close);
        // The frames are driven by the editor window, which main keeps
        // painting while this window is open.
        this.frameHandle = requestAnimationFrame(this.frame);
        // The editor keeps the keyboard.
        window.focus();
        this.notify();
    }

    close = (): void => {
        const { child } = this;
        this.release();
        child?.close();
    };

    /** Takes a newly rendered video frame for the window. */
    show(source: HTMLCanvasElement): void {
        if (this.child?.closed) this.release();
        const { videoContext } = this;
        if (this.canvas === null || videoContext === null) return;
        if (
            this.video.width !== source.width ||
            this.video.height !== source.height
        ) {
            this.video.width = source.width;
            this.video.height = source.height;
        }
        videoContext.drawImage(source, 0, 0);
        this.videoFresh = true;
    }

    private frame = (nowMs: number): void => {
        this.frameHandle = requestAnimationFrame(this.frame);
        const { canvas, child, context } = this;
        const size = this.size;
        if (canvas === null || child === null || context === null) return;
        if (size === null) return;
        let changed = this.stale || this.videoFresh;
        this.stale = false;
        this.videoFresh = false;
        if (canvas.width !== size.width || canvas.height !== size.height) {
            canvas.width = size.width;
            canvas.height = size.height;
            changed = true;
        }
        if (this.lastMs < 0) this.codeAlpha = this.codeTarget;
        const elapsedMs = this.lastMs < 0 ? 0 : nowMs - this.lastMs;
        this.lastMs = nowMs;
        if (this.codeAlpha !== this.codeTarget) {
            const step = elapsedMs / CODE_FADE_MS;
            this.codeAlpha =
                this.codeAlpha < this.codeTarget
                    ? Math.min(this.codeTarget, this.codeAlpha + step)
                    : Math.max(this.codeTarget, this.codeAlpha - step);
            changed = true;
        }
        const { code } = this;
        const showCode = code !== null && this.codeAlpha > 0;
        if (showCode) {
            const pixelRatio = child.devicePixelRatio * this.scale;
            changed =
                code.render(size.width, size.height, pixelRatio, nowMs) ||
                changed;
        }
        if (!changed) return;

        context.fillStyle = '#000';
        context.fillRect(0, 0, size.width, size.height);
        if (this.video.width > 0 && this.video.height > 0) {
            context.drawImage(this.video, 0, 0, size.width, size.height);
        }
        if (showCode) {
            context.globalAlpha = this.codeAlpha;
            context.drawImage(code.canvas, 0, 0);
            context.globalAlpha = 1;
        }
        for (const listener of this.compositeListeners) listener(canvas);
    };

    private release = (): void => {
        if (this.child === null) return;
        cancelAnimationFrame(this.frameHandle);
        this.child.removeEventListener('resize', this.notify);
        this.child.removeEventListener('pagehide', this.release);
        window.removeEventListener('pagehide', this.close);
        this.child = null;
        this.canvas = null;
        this.context = null;
        this.video.width = 0;
        this.video.height = 0;
        this.notify();
    };

    private notify = (): void => {
        for (const listener of this.listeners) listener();
    };
}

export const performanceOutput = new PerformanceOutput();
