import { useState } from 'react';
import type { BundledFont, SystemFont } from '../../shared/ipcTypes';

export const BUNDLED_FONTS: BundledFont[] = [
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
];
const SYSTEM_FONTS: SystemFont[] = ['SF Mono', 'Monaco', 'Menlo', 'Consolas'];

/**
 * Detect whether a font is installed by measuring text against baseline fonts.
 * If the candidate font renders at a different width than all baselines, it's installed.
 */
function isFontInstalled(fontName: string): boolean {
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    if (!ctx) {
        return false;
    }

    const testString = 'mmmmmmmmmmlli1WWW@#$';
    const baselines = ['monospace', 'sans-serif', 'serif'] as const;
    const size = '72px';

    for (const baseline of baselines) {
        ctx.font = `${size} ${baseline}`;
        const baselineWidth = ctx.measureText(testString).width;

        ctx.font = `${size} "${fontName}", ${baseline}`;
        const candidateWidth = ctx.measureText(testString).width;

        if (candidateWidth !== baselineWidth) {
            return true;
        }
    }
    return false;
}

/** The system monospace fonts installed on this machine, measured once. */
export function useInstalledSystemFonts(): SystemFont[] {
    const [installed] = useState(() => SYSTEM_FONTS.filter(isFontInstalled));
    return installed;
}
