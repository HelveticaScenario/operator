import { describe, expect, it } from 'vitest';
import { pickCamera } from './pickCamera';

const devices = [
    { deviceId: 'mic', kind: 'audioinput', label: 'Built-in Microphone' },
    { deviceId: 'a', kind: 'videoinput', label: 'FaceTime HD Camera' },
    { deviceId: 'b', kind: 'videoinput', label: 'Logitech BRIO' },
];

describe('pickCamera', () => {
    it('takes the first camera when none is named', () => {
        expect(pickCamera(devices, undefined)).toEqual(devices[1]);
    });

    it('takes the camera whose name contains the one given, without regard to case', () => {
        expect(pickCamera(devices, 'brio').deviceId).toBe('b');
        expect(pickCamera(devices, 'FACETIME').deviceId).toBe('a');
    });

    it('ignores devices that are not cameras', () => {
        expect(() => pickCamera(devices, 'microphone')).toThrow(
            /no camera is named like "microphone"/,
        );
    });

    it('lists the cameras that exist when none matches', () => {
        expect(() => pickCamera(devices, 'kinect')).toThrow(
            /"FaceTime HD Camera", "Logitech BRIO"/,
        );
    });

    it('says so when no camera is connected', () => {
        expect(() => pickCamera([devices[0]], undefined)).toThrow(
            /no camera is connected/,
        );
    });
});
