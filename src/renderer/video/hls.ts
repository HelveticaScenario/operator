import type {
    HlsConfig,
    Loader,
    LoaderCallbacks,
    LoaderConfiguration,
    LoaderContext,
} from 'hls.js';
import { mediaUrl } from '../../shared/video/mediaUrl';

/** A loader class, as hls.js builds them from its configuration. */
type LoaderClass = new (config: HlsConfig) => Loader<LoaderContext>;

/**
 * A loader that fetches everything hls.js asks for through the app's media
 * scheme, which adds the CORS headers a shader needs and passes byte ranges
 * on. The playlist and segments are still known to hls.js by their own URLs,
 * so relative references resolve against the stream's own address.
 */
export function proxyLoader(Base: LoaderClass): LoaderClass {
    return class ProxyLoader extends Base {
        override load(
            context: LoaderContext,
            config: LoaderConfiguration,
            callbacks: LoaderCallbacks<LoaderContext>,
        ): void {
            const original = context.url;
            super.load({ ...context, url: mediaUrl(original) }, config, {
                ...callbacks,
                onSuccess: (response, stats, _fetched, details) =>
                    callbacks.onSuccess(
                        { ...response, url: original },
                        stats,
                        context,
                        details,
                    ),
            });
        }
    };
}

/**
 * Plays the HLS stream at `url` in `video`. Resolves with a function that
 * stops it. Errors that end playback go to `onError`.
 */
export async function attachHls(
    video: HTMLVideoElement,
    url: string,
    onError: (error: Error) => void,
): Promise<() => void> {
    const { default: Hls } = await import('hls.js');
    if (!Hls.isSupported()) {
        throw new Error('this browser engine cannot play HLS streams');
    }
    const hls = new Hls({
        loader: proxyLoader(Hls.DefaultConfig.loader as unknown as LoaderClass),
    });
    hls.on(Hls.Events.ERROR, (_event, data) => {
        if (data.fatal) {
            onError(new Error(`the stream failed (${data.details})`));
        }
    });
    hls.loadSource(url);
    hls.attachMedia(video);
    return () => hls.destroy();
}
