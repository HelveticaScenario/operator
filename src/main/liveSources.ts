import { app, desktopCapturer, screen, systemPreferences } from 'electron';

/**
 * Whether the app may use the camera, asking the system when it has not been
 * asked. Chromium's fake-device mode never reaches the real camera, so it needs
 * no permission.
 */
export async function requestCameraAccess(): Promise<boolean> {
    if (
        process.platform !== 'darwin' ||
        app.commandLine.hasSwitch('use-fake-ui-for-media-stream')
    ) {
        return true;
    }
    const status = systemPreferences.getMediaAccessStatus('camera');
    if (status === 'granted') return true;
    if (status === 'denied' || status === 'restricted') return false;
    return systemPreferences.askForMediaAccess('camera');
}

/** The capture source of display number `display` (from 1), or why there is none. */
export function pickScreenSource(
    sources: { display_id: string; id: string }[],
    displays: { id: number }[],
    display: number,
): { id: string } | { error: string } {
    const target = displays[display - 1];
    if (target === undefined) {
        return {
            error: `there is no display ${display}; ${displays.length === 1 ? 'there is 1 display' : `there are ${displays.length} displays`}`,
        };
    }
    const source =
        sources.find((s) => s.display_id === String(target.id)) ??
        sources[display - 1];
    return source === undefined
        ? { error: `display ${display} cannot be captured` }
        : { id: source.id };
}

/** The capture source for a display, once the system lets the app record the screen. */
export async function screenSource(
    display: number,
): Promise<{ id: string } | { error: string }> {
    if (
        process.platform === 'darwin' &&
        systemPreferences.getMediaAccessStatus('screen') !== 'granted'
    ) {
        // Asking for sources is what makes the system list the app under
        // Screen Recording, so the user can allow it there.
        await desktopCapturer
            .getSources({ types: ['screen'] })
            .catch(() => undefined);
        return {
            error: 'screen recording is not allowed; allow Operator under System Settings, Privacy & Security, Screen Recording, then restart it',
        };
    }
    const sources = await desktopCapturer.getSources({
        thumbnailSize: { height: 0, width: 0 },
        types: ['screen'],
    });
    return pickScreenSource(sources, screen.getAllDisplays(), display);
}
