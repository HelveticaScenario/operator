import * as fs from 'fs';
import * as path from 'path';
import { z } from 'zod';

const AppConfigSchema = z.object({
    audioConfig: z
        .object({
            hostId: z.string().optional(),
            inputDeviceId: z.string().nullable().optional(),
            outputDeviceId: z.string().optional(),
            sampleRate: z.number().optional(),
            bufferSize: z.number().optional(),
        })
        .optional(),
    cursorStyle: z
        .enum([
            'line',
            'block',
            'underline',
            'line-thin',
            'block-outline',
            'underline-thin',
        ])
        .optional(),
    font: z
        .enum([
            // Bundled fonts
            'Fira Code',
            'JetBrains Mono',
            'Cascadia Code',
            'Source Code Pro',
            'IBM Plex Mono',
            'Hack',
            'Inconsolata',
            'Monaspace Neon',
            'Monaspace Argon',
            'Monaspace Xenon',
            'Monaspace Krypton',
            'Monaspace Radon',
            'Geist Mono',
            'Iosevka',
            'Victor Mono',
            'Roboto Mono',
            'Maple Mono',
            'Commit Mono',
            '0xProto',
            'Intel One Mono',
            'Mononoki',
            'Anonymous Pro',
            'Recursive',
            // System fonts (available only if installed)
            'SF Mono',
            'Monaco',
            'Menlo',
            'Consolas',
        ])
        .optional(),
    fontLigatures: z.boolean().optional(),
    fontSize: z.number().min(8).max(72).optional(),
    lastOpenedFolder: z.string().optional(),
    prettier: z.record(z.string(), z.unknown()).optional(),
    skippedUpdateVersion: z.string().optional(),
    theme: z.string().optional(),
    xyScopeIntensity: z.number().min(0).max(1).optional(),
    xyScopePersistence: z.number().min(0).max(1).optional(),
    xyScopeUpsample: z.boolean().optional(),
    xyScopeLineWidth: z.number().min(0.002).max(0.06).optional(),
    vuPanelVisible: z.boolean().optional(),
    vuPanelHeight: z.number().min(84).max(480).optional(),
});

export type AppConfig = z.infer<typeof AppConfigSchema>;

const DEFAULT_CONFIG: AppConfig = {
    cursorStyle: 'block',
    font: 'Fira Code',
    fontSize: 17,
    theme: 'modular-dark',
};

function isPlainObject(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/**
 * Validate config JSON field by field, recursing into object-valued fields:
 * a leaf that fails its schema (config.json is user-editable, so any value
 * can appear) is dropped while every other field — including siblings inside
 * the same object — survives, and keys the schema does not know, at any
 * depth, pass through untouched (e.g. ones written by a newer build, which
 * update() re-persists verbatim).
 */
function parseFields(
    shape: Record<string, z.ZodType | undefined>,
    json: Record<string, unknown>,
    keyPrefix: string,
): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(json)) {
        const fieldSchema = shape[key];
        if (!fieldSchema) {
            out[key] = value;
            continue;
        }
        const unwrapped =
            fieldSchema instanceof z.ZodOptional
                ? fieldSchema.unwrap()
                : fieldSchema;
        if (unwrapped instanceof z.ZodObject && isPlainObject(value)) {
            out[key] = parseFields(
                unwrapped.shape,
                value,
                `${keyPrefix}${key}.`,
            );
            continue;
        }
        const result = fieldSchema.safeParse(value);
        if (result.success) {
            out[key] = result.data;
        } else {
            console.error(
                `Ignoring invalid config field "${keyPrefix}${key}":`,
                value,
            );
        }
    }
    return out;
}

/** Returns null when the root is not a JSON object. */
export function parseConfig(json: unknown): AppConfig | null {
    if (!isPlainObject(json)) {
        return null;
    }
    return parseFields(AppConfigSchema.shape, json, '') as AppConfig;
}

export interface ConfigStore {
    /** Read the config; an unreadable or missing file reads as `{}`. */
    load(): AppConfig;
    save(config: AppConfig): void;
    /**
     * Load-mutate-save. Skipped entirely when the file exists but cannot be
     * read, so a parse failure never causes the settings on disk to be
     * overwritten with an empty config.
     */
    update(mutate: (config: AppConfig) => void): void;
    /** Write the default config if the file does not exist yet. */
    ensureExists(): void;
    /**
     * Notify on every change to the config file, including atomic saves
     * (write-temp-then-rename) from external editors.
     */
    watch(onChange: (config: AppConfig) => void): fs.FSWatcher;
}

export function createConfigStore(configFile: string): ConfigStore {
    // null means the file exists but is unreadable (unparseable JSON or a
    // non-object root); a missing file reads as {}.
    function read(): AppConfig | null {
        try {
            if (!fs.existsSync(configFile)) {
                return {};
            }
            const data = fs.readFileSync(configFile, 'utf-8');
            return parseConfig(JSON.parse(data));
        } catch (error) {
            console.error('Error loading config:', error);
            return null;
        }
    }

    function load(): AppConfig {
        return read() ?? {};
    }

    function save(config: AppConfig): void {
        try {
            fs.writeFileSync(
                configFile,
                JSON.stringify(config, null, 2),
                'utf-8',
            );
        } catch (error) {
            console.error('Error saving config:', error);
        }
    }

    function update(mutate: (config: AppConfig) => void): void {
        const config = read();
        if (config === null) {
            console.error(
                'config.json is unreadable; leaving it untouched instead of overwriting it',
            );
            return;
        }
        mutate(config);
        save(config);
    }

    function ensureExists(): void {
        if (!fs.existsSync(configFile)) {
            save(DEFAULT_CONFIG);
        }
    }

    // Watch the containing directory rather than the file itself: an atomic
    // save (write-temp-then-rename) replaces the file's inode, which detaches
    // a per-file watcher permanently.
    function watch(onChange: (config: AppConfig) => void): fs.FSWatcher {
        const dir = path.dirname(configFile);
        const base = path.basename(configFile);
        return fs.watch(dir, (_eventType, filename) => {
            // Some platforms omit the filename; treat that as a possible hit.
            if (filename && filename !== base) {
                return;
            }
            const config = read();
            // A mid-write read can fail to parse; skip it rather than pushing
            // an empty config to subscribers.
            if (config !== null) {
                onChange(config);
            }
        });
    }

    return { ensureExists, load, save, update, watch };
}
