/**
 * The camera named like `device` (part of its label, without regard to case),
 * or the first camera when none is named.
 */
export function pickCamera(
    devices: { deviceId: string; kind: string; label: string }[],
    device: string | undefined,
): { deviceId: string } {
    const cameras = devices.filter((d) => d.kind === 'videoinput');
    if (cameras.length === 0) throw new Error('no camera is connected');
    if (device === undefined) return cameras[0];
    const wanted = device.toLowerCase();
    const chosen = cameras.find((c) => c.label.toLowerCase().includes(wanted));
    if (chosen === undefined) {
        throw new Error(
            `no camera is named like "${device}"; the cameras are ${cameras.map((c) => `"${c.label || 'unnamed'}"`).join(', ')}`,
        );
    }
    return chosen;
}
