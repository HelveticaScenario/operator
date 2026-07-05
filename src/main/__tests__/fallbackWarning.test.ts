import { describe, expect, test } from 'vitest';
import { createFallbackWarningChannel } from '../fallbackWarning';

describe('createFallbackWarningChannel', () => {
    test('a warning reported before a window exists is delivered on attach', () => {
        const channel = createFallbackWarningChannel();
        channel.report('Saved output device not found; using default');

        const delivered: string[] = [];
        channel.attach((warning) => delivered.push(warning));
        expect(delivered).toEqual([
            'Saved output device not found; using default',
        ]);
    });

    test('a held warning is flushed only once', () => {
        const channel = createFallbackWarningChannel();
        channel.report('fallback');

        const delivered: string[] = [];
        const send = (warning: string) => delivered.push(warning);
        channel.attach(send);
        channel.attach(send);
        expect(delivered).toEqual(['fallback']);
    });

    test('warnings reported after attach are delivered immediately', () => {
        const channel = createFallbackWarningChannel();
        const delivered: string[] = [];
        channel.attach((warning) => delivered.push(warning));

        channel.report('device unplugged');
        expect(delivered).toEqual(['device unplugged']);
    });

    test('empty and missing warnings are ignored', () => {
        const channel = createFallbackWarningChannel();
        const delivered: string[] = [];
        channel.attach((warning) => delivered.push(warning));

        channel.report(undefined);
        channel.report(null);
        channel.report('');
        expect(delivered).toEqual([]);
    });
});
