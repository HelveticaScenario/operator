import { describe, expect, it, vi } from 'vitest';
import type {
    HlsConfig,
    LoaderCallbacks,
    LoaderConfiguration,
    LoaderContext,
    LoaderResponse,
} from 'hls.js';
import { proxyLoader } from './hls';

/** Records what the loader it extends is asked to do. */
class FakeLoader {
    static calls: {
        callbacks: LoaderCallbacks<LoaderContext>;
        context: LoaderContext;
    }[] = [];
    context = null;
    stats = {} as never;
    destroy() {}
    abort() {}
    load(
        context: LoaderContext,
        _config: LoaderConfiguration,
        callbacks: LoaderCallbacks<LoaderContext>,
    ) {
        FakeLoader.calls.push({ callbacks, context });
    }
}

const remote = 'https://cdn.example.com/live/index.m3u8';

describe('proxyLoader', () => {
    const Loader = proxyLoader(FakeLoader as never);
    const request = () => {
        FakeLoader.calls.length = 0;
        const callbacks = {
            onError: vi.fn(),
            onSuccess: vi.fn(),
            onTimeout: vi.fn(),
        } as unknown as LoaderCallbacks<LoaderContext>;
        const context = { responseType: 'text', url: remote } as LoaderContext;
        new Loader({} as HlsConfig).load(
            context,
            {} as LoaderConfiguration,
            callbacks,
        );
        return { callbacks, context, sent: FakeLoader.calls[0] };
    };

    it('asks for the address through the app', () => {
        const { sent } = request();
        expect(sent.context.url).toBe(
            `operator-media://remote/${encodeURIComponent(remote)}`,
        );
        expect(sent.context.responseType).toBe('text');
    });

    it('reports a response as coming from the address, so relative segments resolve against it', () => {
        const { callbacks, context, sent } = request();
        const response: LoaderResponse = {
            data: '#EXTM3U',
            url: sent.context.url,
        };
        sent.callbacks.onSuccess(response, {} as never, sent.context, null);
        const [reported, , reportedContext] = (
            callbacks.onSuccess as ReturnType<typeof vi.fn>
        ).mock.calls[0];
        expect(reported.url).toBe(remote);
        expect(reported.data).toBe('#EXTM3U');
        expect(reportedContext).toBe(context);
    });

    it('passes failures on as they come', () => {
        const { callbacks, sent } = request();
        sent.callbacks.onError(
            { code: 404, text: 'no' },
            sent.context,
            null,
            {} as never,
        );
        expect(callbacks.onError).toHaveBeenCalled();
    });
});
