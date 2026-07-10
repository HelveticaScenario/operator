/**
 * JSON Schema for the Modular app configuration file.
 * Used by Monaco editor for validation and autocomplete.
 */

const audioConfigSchema = {
    additionalProperties: false,
    description: 'Audio device configuration',
    properties: {
        bufferSize: {
            description: 'Buffer size in samples (e.g., 256, 512)',
            type: 'number',
        },
        hostId: {
            description: "Audio host identifier (e.g., 'CoreAudio', 'WASAPI')",
            type: 'string',
        },
        inputDeviceId: {
            description: 'Input device ID, or null for no input',
            type: ['string', 'null'],
        },
        outputDeviceId: {
            description: 'Output device ID',
            type: 'string',
        },
        sampleRate: {
            description: 'Sample rate in Hz (e.g., 44100, 48000)',
            type: 'number',
        },
    },
    type: 'object',
};

export const configSchema = {
    $schema: 'http://json-schema.org/draft-07/schema#',
    additionalProperties: false,
    description: 'Configuration file for the Operator synthesizer application',
    properties: {
        audioConfig: audioConfigSchema,
        cursorStyle: {
            default: 'block',
            description: 'The cursor style in the editor',
            enum: [
                'line',
                'block',
                'underline',
                'line-thin',
                'block-outline',
                'underline-thin',
            ],
            type: 'string',
        },
        font: {
            default: 'Fira Code',
            description: 'The monospace font family used in the editor',
            enum: [
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
                'SF Mono',
                'Monaco',
                'Menlo',
                'Consolas',
            ],
            type: 'string',
        },
        fontLigatures: {
            default: true,
            description: 'Enable or disable font ligatures in the editor',
            type: 'boolean',
        },
        fontSize: {
            default: 17,
            description: 'The font size in the editor (8–72)',
            maximum: 72,
            minimum: 8,
            type: 'number',
        },
        lastOpenedFolder: {
            description:
                'Path to the last opened workspace folder (managed automatically)',
            type: 'string',
        },
        prettier: {
            additionalProperties: true,
            description:
                "Prettier formatting options. Merged with defaults: singleQuote=true, trailingComma='all', semi=false, tabWidth=2, printWidth=60. The parser and plugins cannot be overridden.",
            properties: {
                printWidth: {
                    default: 60,
                    description: 'Line width before wrapping',
                    type: 'number',
                },
                semi: {
                    default: false,
                    description: 'Add semicolons at the end of statements',
                    type: 'boolean',
                },
                singleQuote: {
                    default: true,
                    description: 'Use single quotes instead of double quotes',
                    type: 'boolean',
                },
                tabWidth: {
                    default: 2,
                    description: 'Number of spaces per indentation level',
                    type: 'number',
                },
                trailingComma: {
                    default: 'all',
                    description: 'Trailing comma style',
                    enum: ['all', 'es5', 'none'],
                    type: 'string',
                },
            },
            type: 'object',
        },
        vuPanelVisible: {
            default: false,
            description: 'Show the VU meter panel at the bottom of the window',
            type: 'boolean',
        },
        vuPanelHeight: {
            default: 150,
            description: 'Height of the VU meter panel in pixels',
            maximum: 480,
            minimum: 84,
            type: 'number',
        },
        theme: {
            default: 'modular-dark',
            description: 'The color theme for the application and editor',
            enum: [
                'modular-dark',
                'one-dark-pro',
                'dracula',
                'gruvbox-dark',
                'tokyo-night',
                'catppuccin-mocha',
            ],
            type: 'string',
        },
    },
    title: 'Operator Configuration',
    type: 'object',
};
