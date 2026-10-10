import schemas from '@modular/core/schemas.json';
import { executePatchScript } from '../../executor';

/** Media audio the tests stand in for: every file has stereo sound but `silent.mp4`. */
export const mediaAudioLoads: string[] = [];
export const loadMediaAudio = (path: string) => {
    mediaAudioLoads.push(path);
    return path.endsWith('silent.mp4')
        ? null
        : {
              barCount: null,
              bitDepth: 32,
              channels: 2,
              cuePoints: [],
              duration: 1,
              frameCount: 48_000,
              loops: [],
              mtime: 1,
              path: `media:${path}`,
              sampleRate: 48_000,
          };
};

/** Runs a patch as the app does, with a workspace and media audio stubbed. */
export const exec = (source: string) =>
    executePatchScript(source, schemas as never, {
        loadMediaAudio,
        sampleRate: 48_000,
        workspaceRoot: '/workspace',
    });
