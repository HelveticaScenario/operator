import { describe, expect, it, vi } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

vi.mock('electron', () => ({ net: {}, protocol: {} }));

const { mediaFileExists, resolveMediaFile } = await import('../mediaProtocol');
const { mediaUrl } = await import('../../shared/video/mediaUrl');

describe('media files', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'media-root-'));
    fs.mkdirSync(path.join(root, 'pictures'));
    fs.writeFileSync(path.join(root, 'pictures', 'photo.png'), 'x');
    const outside = path.join(path.dirname(root), 'secret.png');
    fs.writeFileSync(outside, 'x');

    it('resolves a path inside the workspace', () => {
        expect(resolveMediaFile(root, mediaUrl('pictures/photo.png'))).toBe(
            path.join(root, 'pictures', 'photo.png'),
        );
    });

    const stays = (resolved: string | null) =>
        resolved === null || resolved.startsWith(root + path.sep);

    it('never resolves outside the workspace folder, however the path is written', () => {
        for (const url of [
            mediaUrl('../secret.png'),
            'operator-media://workspace/%2e%2e/secret.png',
            'operator-media://workspace/%2E%2E%2Fsecret.png',
            mediaUrl('pictures/../../secret.png'),
            'operator-media://workspace/..%5Csecret.png',
        ]) {
            const resolved = resolveMediaFile(root, url);
            expect(stays(resolved)).toBe(true);
            expect(resolved).not.toBe(outside);
        }
    });

    it('refuses the workspace folder itself, a bad URL, or no workspace', () => {
        expect(
            resolveMediaFile(root, 'operator-media://workspace/'),
        ).toBeNull();
        expect(resolveMediaFile(root, 'https://example.com/a.png')).toBeNull();
        expect(
            resolveMediaFile(null, mediaUrl('pictures/photo.png')),
        ).toBeNull();
    });

    it('does not mistake a sibling folder with the same prefix for the workspace', () => {
        const sibling = `${root}-other`;
        fs.mkdirSync(sibling, { recursive: true });
        fs.writeFileSync(path.join(sibling, 'a.png'), 'x');
        const resolved = resolveMediaFile(
            root,
            mediaUrl(`../${path.basename(sibling)}/a.png`),
        );
        expect(stays(resolved)).toBe(true);
        expect(resolved).not.toBe(path.join(sibling, 'a.png'));
    });

    it('reports whether a workspace file exists', () => {
        expect(mediaFileExists(root, 'pictures/photo.png')).toBe(true);
        expect(mediaFileExists(root, 'pictures/missing.png')).toBe(false);
        expect(mediaFileExists(root, '../secret.png')).toBe(false);
        expect(mediaFileExists(null, 'pictures/photo.png')).toBe(false);
    });
});
