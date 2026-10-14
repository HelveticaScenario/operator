import type { VideoSourceDef } from '../../shared/video/videoGraph';
import electronAPI from '../electronAPI';
import { pickCamera } from './pickCamera';

/** Camera frames asked of the device, which gives the nearest it can. */
const CAMERA_SIZE = { height: { ideal: 1080 }, width: { ideal: 1920 } };

async function openCamera(device: string | undefined): Promise<MediaStream> {
    if (!(await electronAPI.video.requestCameraAccess())) {
        throw new Error(
            'camera access is not allowed; allow Operator under System Settings, Privacy & Security, Camera, then restart it',
        );
    }
    if (device === undefined) {
        return navigator.mediaDevices.getUserMedia({
            audio: false,
            video: CAMERA_SIZE,
        });
    }
    // Cameras have no names until the page has been given one.
    const probe = await navigator.mediaDevices.getUserMedia({ video: true });
    for (const track of probe.getTracks()) track.stop();
    const { deviceId } = pickCamera(
        await navigator.mediaDevices.enumerateDevices(),
        device,
    );
    return navigator.mediaDevices.getUserMedia({
        audio: false,
        video: { ...CAMERA_SIZE, deviceId: { exact: deviceId } },
    });
}

async function openScreen(display: number): Promise<MediaStream> {
    const source = await electronAPI.video.getScreenSource(display);
    if ('error' in source) throw new Error(source.error);
    // Electron's own constraint form for capturing a desktop source.
    return navigator.mediaDevices.getUserMedia({
        audio: false,
        video: {
            mandatory: {
                chromeMediaSource: 'desktop',
                chromeMediaSourceId: source.id,
            },
        },
    } as MediaStreamConstraints);
}

/** The stream of a camera or a screen, for the source textures to show. */
export function openLiveSource(def: VideoSourceDef): Promise<MediaStream> {
    if (def.kind === 'camera') return openCamera(def.device);
    if (def.kind === 'screen') return openScreen(def.display ?? 1);
    return Promise.reject(new Error(`${def.kind} is not a live source`));
}
