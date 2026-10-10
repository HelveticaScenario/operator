import { describe, expect, it } from 'vitest';
import { mediaUrl, parseMediaUrl } from './mediaUrl';

describe('mediaUrl', () => {
    it('round-trips a path, including characters that need escaping', () => {
        for (const path of [
            'photo.png',
            'pictures/photo.png',
            'my clips/loop #2 (final).mp4',
            'ünïcode/ファイル.webp',
            'a%20b/c.png',
        ]) {
            expect(parseMediaUrl(mediaUrl(path))).toBe(path);
        }
    });

    it('escapes each segment but keeps the separators', () => {
        expect(mediaUrl('a b/c d.png')).toBe(
            'operator-media://workspace/a%20b/c%20d.png',
        );
    });

    it('rejects a URL of another scheme or host', () => {
        expect(parseMediaUrl('https://workspace/a.png')).toBeNull();
        expect(parseMediaUrl('operator-media://elsewhere/a.png')).toBeNull();
        expect(parseMediaUrl('file:///etc/passwd')).toBeNull();
        expect(parseMediaUrl('not a url')).toBeNull();
    });

    it('rejects broken escapes', () => {
        expect(
            parseMediaUrl('operator-media://workspace/%E0%A4%A.png'),
        ).toBeNull();
    });

    it('has parent segments collapsed by the URL parser, even when escaped', () => {
        expect(parseMediaUrl('operator-media://workspace/a/%2e%2e/b.png')).toBe(
            'b.png',
        );
        expect(parseMediaUrl(mediaUrl('../../x.png'))).toBe('x.png');
    });
});
