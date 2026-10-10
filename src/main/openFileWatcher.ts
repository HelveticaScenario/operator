import * as fs from 'fs';
import * as path from 'path';

const DEBOUNCE_MS = 50;
const RETRY_MS = 1000;

export interface OpenFileWatcher {
    /** Replace the watched set with these absolute file paths. */
    setFiles(filePaths: string[]): void;
    close(): void;
}

/**
 * Watch a set of files for changes made by other programs. Each file's parent
 * directory is watched rather than the file itself: editors that save by
 * writing a temp file and renaming it over the original replace the inode, and
 * a watch on the file would go silent after the first such save.
 *
 * `onChange` fires once per burst of events on a file, including events caused
 * by this process's own writes — callers compare content to tell them apart.
 *
 * A directory whose watch fails or that disappears is retried every RETRY_MS
 * while any of its files is still watched. Once the watch is back, `onChange`
 * fires for each of its files, since changes in between went unseen.
 */
export function createOpenFileWatcher(
    onChange: (filePath: string) => void,
): OpenFileWatcher {
    const filesByDir = new Map<string, Set<string>>();
    const dirWatchers = new Map<string, fs.FSWatcher>();
    const timers = new Map<string, NodeJS.Timeout>();
    const retryTimers = new Map<string, NodeJS.Timeout>();

    const schedule = (filePath: string) => {
        clearTimeout(timers.get(filePath));
        timers.set(
            filePath,
            setTimeout(() => {
                timers.delete(filePath);
                if (watchedFile(filePath)) {
                    onChange(filePath);
                }
            }, DEBOUNCE_MS),
        );
    };

    const watchedFile = (filePath: string) =>
        filesByDir.get(path.dirname(filePath))?.has(filePath) ?? false;

    const unwatchDir = (dir: string) => {
        dirWatchers.get(dir)?.close();
        dirWatchers.delete(dir);
        clearTimeout(retryTimers.get(dir));
        retryTimers.delete(dir);
    };

    const retryLater = (dir: string) => {
        unwatchDir(dir);
        retryTimers.set(
            dir,
            setTimeout(() => {
                retryTimers.delete(dir);
                const files = filesByDir.get(dir);
                if (!files) {
                    return;
                }
                if (watchDir(dir)) {
                    files.forEach(schedule);
                } else {
                    retryLater(dir);
                }
            }, RETRY_MS),
        );
    };

    /** Start watching `dir`; false when it cannot be watched right now. */
    const watchDir = (dir: string): boolean => {
        try {
            const watcher = fs.watch(dir, (_eventType, filename) => {
                const files = filesByDir.get(dir);
                if (!files) {
                    return;
                }
                // Some platforms keep a watch open, but silent, after its
                // directory is deleted, so check on every event.
                if (!fs.existsSync(dir)) {
                    retryLater(dir);
                    return;
                }
                // Some platforms omit the filename; then any file in the
                // directory may have changed.
                if (filename === null) {
                    files.forEach(schedule);
                    return;
                }
                const filePath = path.join(dir, filename.toString());
                if (files.has(filePath)) {
                    schedule(filePath);
                }
            });
            watcher.on('error', () => retryLater(dir));
            dirWatchers.set(dir, watcher);
            return true;
        } catch {
            return false;
        }
    };

    return {
        setFiles(filePaths) {
            filesByDir.clear();
            for (const filePath of filePaths) {
                const resolved = path.resolve(filePath);
                const dir = path.dirname(resolved);
                let files = filesByDir.get(dir);
                if (!files) {
                    files = new Set();
                    filesByDir.set(dir, files);
                }
                files.add(resolved);
            }
            for (const dir of [...dirWatchers.keys(), ...retryTimers.keys()]) {
                if (!filesByDir.has(dir)) {
                    unwatchDir(dir);
                }
            }
            for (const dir of filesByDir.keys()) {
                if (
                    !dirWatchers.has(dir) &&
                    !retryTimers.has(dir) &&
                    !watchDir(dir)
                ) {
                    retryLater(dir);
                }
            }
        },
        close() {
            filesByDir.clear();
            [...dirWatchers.keys(), ...retryTimers.keys()].forEach(unwatchDir);
            timers.forEach((timer) => clearTimeout(timer));
            timers.clear();
        },
    };
}
