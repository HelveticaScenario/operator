import { MONOSPACE_FONTS } from '../shared/fonts';

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

const THEMES = [
    'modular-dark',
    'one-dark-pro',
    'dracula',
    'gruvbox-dark',
    'tokyo-night',
    'catppuccin-mocha',
];

export const configSchema = {
    $schema: 'http://json-schema.org/draft-07/schema#',
    additionalProperties: false,
    description: 'Configuration file for the Operator synthesizer application',
    properties: {
        audioConfig: audioConfigSchema,
        codeBackdropOpacity: {
            default: 0.6,
            description:
                'Opacity of the backing behind the code while video plays (0–1)',
            maximum: 1,
            minimum: 0,
            type: 'number',
        },
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
            enum: [...MONOSPACE_FONTS],
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
        performanceAspect: {
            default: 'free',
            description:
                'Shape of the performance window; free lets it take any size',
            enum: ['free', '16:9', '4:3', '1:1', '9:16'],
            type: 'string',
        },
        performanceScale: {
            default: 1,
            description:
                'Resolution of the performance window as a fraction of its size (0.1–1)',
            maximum: 1,
            minimum: 0.1,
            type: 'number',
        },
        performanceCodeBacking: {
            default: 0.6,
            description:
                'Opacity of the backing behind each line of code in the performance window (0–1)',
            maximum: 1,
            minimum: 0,
            type: 'number',
        },
        performanceCodeOpacity: {
            default: 1,
            description:
                'Opacity of the code in the performance window while it shows (0–1)',
            maximum: 1,
            minimum: 0,
            type: 'number',
        },
        performanceCodeVisible: {
            default: true,
            description:
                'Show the code in the performance window; View, Toggle Performance Code fades it in and out',
            type: 'boolean',
        },
        performanceFont: {
            default: 'Fira Code',
            description:
                'The monospace font family of the code in the performance window',
            enum: [...MONOSPACE_FONTS],
            type: 'string',
        },
        performanceFontLigatures: {
            default: true,
            description: 'Enable font ligatures in the performance window',
            type: 'boolean',
        },
        performanceFontSize: {
            default: 28,
            description:
                'The font size of the code in the performance window (8–160)',
            maximum: 160,
            minimum: 8,
            type: 'number',
        },
        performanceLineHeight: {
            default: 1.4,
            description:
                'Line height of the code in the performance window, as a multiple of its font size (1–3)',
            maximum: 3,
            minimum: 1,
            type: 'number',
        },
        performanceScrollSpeed: {
            default: 0.5,
            description:
                'How quickly the code in the performance window scrolls to follow the editor (0–1); 1 jumps',
            maximum: 1,
            minimum: 0,
            type: 'number',
        },
        performanceTheme: {
            description:
                "The color theme of the code in the performance window; absent follows the editor's theme",
            enum: THEMES,
            type: 'string',
        },
        performanceLineNumbers: {
            default: false,
            description: 'Show line numbers in the performance window',
            type: 'boolean',
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
        skippedUpdateVersion: {
            description:
                'Release version whose update prompt was dismissed (managed automatically)',
            type: 'string',
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
            enum: THEMES,
            type: 'string',
        },
    },
    title: 'Operator Configuration',
    type: 'object',
};
