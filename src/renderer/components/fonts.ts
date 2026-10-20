import { useState } from 'react';
import { SYSTEM_FONTS, type SystemFont } from '../../shared/fonts';

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
