import fs from 'node:fs/promises';
import path from 'node:path';

function hostNapiPackageInfo() {
    const platform = process.platform;
    const arch = process.arch;

    if (platform === 'darwin' && arch === 'arm64') {
        return {
            folder: 'darwin-arm64',
            filename: 'modular.darwin-arm64.node',
        };
    }
    if (platform === 'darwin' && arch === 'x64') {
        return { folder: 'darwin-x64', filename: 'modular.darwin-x64.node' };
    }
    if (platform === 'linux' && arch === 'x64') {
        return {
            folder: 'linux-x64-gnu',
            filename: 'modular.linux-x64-gnu.node',
        };
    }
    if (platform === 'win32' && arch === 'x64') {
        return {
            folder: 'win32-x64-msvc',
            filename: 'modular.win32-x64-msvc.node',
        };
    }

    throw new Error(
        `Unsupported host for local npm/* packaging: ${platform} ${arch}`,
    );
}

const { folder, filename } = hostNapiPackageInfo();

const repoRoot = path.resolve(new URL('.', import.meta.url).pathname, '..');
const src = path.join(repoRoot, filename);
const destDir = path.join(repoRoot, 'npm', folder);
const dest = path.join(destDir, filename);

await fs.mkdir(destDir, { recursive: true });
await fs.copyFile(src, dest);

process.stdout.write(`Copied ${filename} -> npm/${folder}/${filename}\n`);
