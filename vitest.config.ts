import { defineConfig } from 'vitest/config';
import path from 'path';

export default defineConfig({
    test: {
        include: [
            'src/**/*.test.ts',
            'src/**/*.test.tsx',
            'crates/modular/__test__/**/*.test.ts',
        ],
        // Native N-API modules don't work with worker threads
        pool: 'forks',
        forks: {
            singleFork: true,
        },
        testTimeout: 30_000,
    },
    resolve: {
        alias: {
            '@modular/core': path.resolve(__dirname, 'crates/modular'),
        },
    },
});
