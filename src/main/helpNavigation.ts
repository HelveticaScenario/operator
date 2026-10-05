/** The subset of Electron's WebContents needed to deliver a navigation. */
export interface HelpNavigationTarget {
    isLoading(): boolean;
    once(event: 'did-finish-load', listener: () => void): unknown;
    send(channel: string, payload: unknown): void;
}

/**
 * Deliver a navigate-to-symbol message to the help window. The message is
 * deferred to `did-finish-load` only while the page is still loading; a
 * loaded window gets it immediately and no listener, so repeat invocations
 * never queue navigations that would replay on the window's next reload.
 */
export function sendNavigateToSymbol(
    webContents: HelpNavigationTarget,
    symbolType: 'type' | 'module' | 'namespace',
    symbolName: string,
): void {
    const send = () => {
        webContents.send('navigate-to-symbol', { symbolName, symbolType });
    };
    if (webContents.isLoading()) {
        webContents.once('did-finish-load', send);
    } else {
        send();
    }
}
