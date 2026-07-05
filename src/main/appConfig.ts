import * as fs from 'fs';
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
});

export type AppConfig = z.infer<typeof AppConfigSchema>;

const DEFAULT_CONFIG: AppConfig = {
    cursorStyle: 'block',
    font: 'Fira Code',
    fontSize: 17,
    theme: 'modular-dark',
};

export interface ConfigStore {
    /** Read the config; an unreadable or missing file reads as `{}`. */
    load(): AppConfig;
    save(config: AppConfig): void;
    /** Write the default config if the file does not exist yet. */
    ensureExists(): void;
}

export function createConfigStore(configFile: string): ConfigStore {
    function load(): AppConfig {
        try {
            if (fs.existsSync(configFile)) {
                const data = fs.readFileSync(configFile, 'utf-8');
                const json = JSON.parse(data);
                const result = AppConfigSchema.safeParse(json);
                if (result.success) {
                    return result.data;
                }
                console.error('Config validation failed:', result.error);
            }
        } catch (error) {
            console.error('Error loading config:', error);
        }
        return {};
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

    function ensureExists(): void {
        if (!fs.existsSync(configFile)) {
            save(DEFAULT_CONFIG);
        }
    }

    return { ensureExists, load, save };
}
