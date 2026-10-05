import { describe, expect, test } from 'vitest';
import {
    sendNavigateToSymbol,
    type HelpNavigationTarget,
} from '../helpNavigation';

/** Minimal WebContents stand-in with a manually fired did-finish-load. */
function fakeWebContents(loading: boolean) {
    const sent: unknown[] = [];
    const listeners: Array<() => void> = [];
    const target: HelpNavigationTarget = {
        isLoading: () => loading,
        once: (_event, listener) => {
            listeners.push(listener);
        },
        send: (_channel, payload) => {
            sent.push(payload);
        },
    };
    const finishLoad = () => {
        const queued = listeners.splice(0);
        for (const listener of queued) {
            listener();
        }
    };
    return { finishLoad, listeners, sent, target };
}

describe('sendNavigateToSymbol', () => {
    test('a loaded window gets the message immediately and no listener', () => {
        const wc = fakeWebContents(false);
        sendNavigateToSymbol(wc.target, 'module', '$lpf');
        expect(wc.sent).toEqual([{ symbolName: '$lpf', symbolType: 'module' }]);
        expect(wc.listeners).toHaveLength(0);
    });

    test('repeat invocations on a loaded window leave nothing to replay on the next load', () => {
        const wc = fakeWebContents(false);
        sendNavigateToSymbol(wc.target, 'module', '$lpf');
        sendNavigateToSymbol(wc.target, 'module', '$sampler');
        expect(wc.sent).toHaveLength(2);

        // A later reload (HMR, Cmd+R) must not re-navigate the window.
        wc.finishLoad();
        expect(wc.sent).toHaveLength(2);
    });

    test('a loading window gets the message once loading finishes', () => {
        const wc = fakeWebContents(true);
        sendNavigateToSymbol(wc.target, 'type', 'ModuleOutput');
        expect(wc.sent).toHaveLength(0);
        wc.finishLoad();
        expect(wc.sent).toEqual([
            { symbolName: 'ModuleOutput', symbolType: 'type' },
        ]);
    });
});
