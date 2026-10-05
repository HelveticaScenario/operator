export interface FallbackWarningChannel {
    /** Forward a warning to the renderer, or hold it until one attaches. */
    report(warning: string | null | undefined): void;
    /** Set the delivery function and flush any held warning through it. */
    attach(send: (warning: string) => void): void;
}

/**
 * Routes Synthesizer audio-device fallback warnings to the renderer. The
 * Synthesizer is constructed before any window exists, so a warning raised at
 * startup is held until a window attaches and delivered on its first load.
 */
export function createFallbackWarningChannel(): FallbackWarningChannel {
    let pending: string | null = null;
    let deliver: ((warning: string) => void) | null = null;
    return {
        report(warning) {
            if (!warning) {
                return;
            }
            if (deliver) {
                deliver(warning);
            } else {
                pending = warning;
            }
        },
        attach(send) {
            deliver = send;
            if (pending !== null) {
                deliver(pending);
                pending = null;
            }
        },
    };
}
