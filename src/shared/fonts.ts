/** Monospace fonts shipped with the app. */
export const BUNDLED_FONTS = [
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
] as const;

/** Monospace fonts used only where the system has them installed. */
export const SYSTEM_FONTS = ['SF Mono', 'Monaco', 'Menlo', 'Consolas'] as const;

/** Every monospace font the config accepts. */
export const MONOSPACE_FONTS = [...BUNDLED_FONTS, ...SYSTEM_FONTS] as const;

export type BundledFont = (typeof BUNDLED_FONTS)[number];
export type SystemFont = (typeof SYSTEM_FONTS)[number];
export type MonospaceFont = (typeof MONOSPACE_FONTS)[number];

/**
 * The fonts with an italic face. Bundled fonts load only their upright faces,
 * and Monaco has none; nothing slants a face that is not italic
 * (font-synthesis: none).
 */
export const ITALIC_FONTS: ReadonlySet<MonospaceFont> = new Set<MonospaceFont>([
    'SF Mono',
    'Menlo',
    'Consolas',
]);
