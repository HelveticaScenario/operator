import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import {
    afterEach,
    beforeEach,
    describe,
    expect,
    type Mock,
    test,
    vi,
} from 'vitest';
import {
    createOpenFileWatcher,
    type OpenFileWatcher,
} from '../openFileWatcher';

let dir: string;
let watcher: OpenFileWatcher | null = null;

beforeEach(() => {
    // Left unresolved: on macOS the temp dir sits behind a /var symlink, as
    // workspaces can.
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'open-file-watcher-'));
});

afterEach(() => {
    watcher?.close();
    watcher = null;
    fs.rmSync(dir, { force: true, recursive: true });
});

const settle = () => new Promise((resolve) => setTimeout(resolve, 300));

/**
 * Start watching, then drop reports of writes made before the watch began —
 * macOS can deliver those after the watcher starts.
 */
async function watchSettled(
    files: string[],
    onChange: Mock<(filePath: string) => void>,
) {
    watcher = createOpenFileWatcher(onChange);
    watcher.setFiles(files);
    await settle();
    onChange.mockClear();
}

describe('createOpenFileWatcher', () => {
    test('reports a write to a watched file', async () => {
        const file = path.join(dir, 'a.mjs');
        fs.writeFileSync(file, 'one');
        const onChange = vi.fn<(filePath: string) => void>();
        await watchSettled([file], onChange);

        fs.writeFileSync(file, 'two');
        await vi.waitFor(() => expect(onChange).toHaveBeenCalledWith(file));
    });

    test('keeps reporting after the file is replaced by a rename', async () => {
        const file = path.join(dir, 'a.mjs');
        fs.writeFileSync(file, 'one');
        const onChange = vi.fn<(filePath: string) => void>();
        await watchSettled([file], onChange);

        const temp = path.join(dir, 'a.mjs.tmp');
        fs.writeFileSync(temp, 'two');
        fs.renameSync(temp, file);
        await vi.waitFor(() => expect(onChange).toHaveBeenCalledWith(file));

        onChange.mockClear();
        fs.writeFileSync(file, 'three');
        await vi.waitFor(() => expect(onChange).toHaveBeenCalledWith(file));
    });

    test('ignores other files in the directory and files no longer watched', async () => {
        const file = path.join(dir, 'a.mjs');
        fs.writeFileSync(file, 'one');
        const onChange = vi.fn<(filePath: string) => void>();
        await watchSettled([file], onChange);

        fs.writeFileSync(path.join(dir, 'other.mjs'), 'x');
        await settle();
        expect(onChange).not.toHaveBeenCalled();

        watcher!.setFiles([]);
        fs.writeFileSync(file, 'two');
        await settle();
        expect(onChange).not.toHaveBeenCalled();
    });

    test('coalesces a burst of writes into one report', async () => {
        const file = path.join(dir, 'a.mjs');
        fs.writeFileSync(file, 'one');
        const onChange = vi.fn<(filePath: string) => void>();
        await watchSettled([file], onChange);

        fs.writeFileSync(file, 'two');
        fs.writeFileSync(file, 'three');
        fs.appendFileSync(file, 'four');
        await settle();
        expect(onChange).toHaveBeenCalledTimes(1);
    });

    test('resumes after the directory is deleted and recreated', async () => {
        const sub = path.join(dir, 'sub');
        const file = path.join(sub, 'a.mjs');
        fs.mkdirSync(sub);
        fs.writeFileSync(file, 'one');
        const onChange = vi.fn<(filePath: string) => void>();
        await watchSettled([file], onChange);

        fs.rmSync(sub, { force: true, recursive: true });
        await settle();
        fs.mkdirSync(sub);
        fs.writeFileSync(file, 'two');
        await vi.waitFor(() => expect(onChange).toHaveBeenCalledWith(file), {
            timeout: 5000,
        });

        // The new directory is watched, not just reported once.
        onChange.mockClear();
        await settle();
        onChange.mockClear();
        fs.writeFileSync(file, 'three');
        await vi.waitFor(() => expect(onChange).toHaveBeenCalledWith(file), {
            timeout: 5000,
        });
    });

    test('starts watching a directory that appears later', async () => {
        const file = path.join(dir, 'later', 'a.mjs');
        const onChange = vi.fn<(filePath: string) => void>();
        watcher = createOpenFileWatcher(onChange);
        watcher.setFiles([file]);

        fs.mkdirSync(path.dirname(file));
        fs.writeFileSync(file, 'one');
        await vi.waitFor(() => expect(onChange).toHaveBeenCalledWith(file), {
            timeout: 5000,
        });
    });

    test('stops retrying once the file is no longer watched', async () => {
        const file = path.join(dir, 'gone', 'a.mjs');
        const onChange = vi.fn<(filePath: string) => void>();
        watcher = createOpenFileWatcher(onChange);
        watcher.setFiles([file]);
        watcher.setFiles([]);

        fs.mkdirSync(path.dirname(file));
        fs.writeFileSync(file, 'one');
        await new Promise((resolve) => setTimeout(resolve, 1500));
        expect(onChange).not.toHaveBeenCalled();
    });
});
