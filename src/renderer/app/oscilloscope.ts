import type { ScopeBufferKey, ScopeChannel, ScopeMode } from '@modular/core';

export const scopeBufferKeyToString = (key: ScopeBufferKey): string => {
    const trigger = key.triggerThreshold
        ? `${key.triggerThreshold[0]}:${key.triggerThreshold[1]}`
        : 'none';
    return `:module:${key.moduleId}:${key.portName}:${key.channel}:${key.msPerFrame}:${trigger}`;
};

export const scopeBufferKeyFromChannel = (
    channel: ScopeChannel,
    msPerFrame: number,
    triggerThreshold?: [number, ScopeMode],
): string => {
    const trigger = triggerThreshold
        ? `${triggerThreshold[0]}:${triggerThreshold[1]}`
        : 'none';
    return `:module:${channel.moduleId}:${channel.portName}:${channel.channel}:${msPerFrame}:${trigger}`;
};

export interface ScopeColors {
    bg: string;
    border: string;
    muted: string;
    accent: string;
}

export function readScopeColors(): ScopeColors {
    const styles = getComputedStyle(document.documentElement);
    return {
        bg: styles.getPropertyValue('--bg-primary').trim() || '#0a0a0a',
        border: styles.getPropertyValue('--border-subtle').trim() || '#222222',
        muted: styles.getPropertyValue('--text-muted').trim() || '#555555',
        accent: styles.getPropertyValue('--accent-primary').trim() || '#4ec9b0',
    };
}

export interface ScopeDrawOptions {
    range: [number, number];
    colors: ScopeColors;
    stats: {
        min: number;
        max: number;
        peakToPeak: number;
        readOffset: number[];
    };
}

export const drawOscilloscope = (
    channels: Float32Array[],
    canvas: HTMLCanvasElement,
    options: ScopeDrawOptions,
) => {
    const ctx = canvas.getContext('2d');
    if (!ctx) {
        return;
    }

    const { range = [-5, 5], stats, colors } = options;
    const [minVoltage, maxVoltage] = range;
    const w = canvas.width;
    const h = canvas.height;

    const bgColor = colors.bg;
    const borderColor = colors.border;
    const mutedColor = colors.muted;
    const accentColor = colors.accent;

    ctx.fillStyle = bgColor;
    ctx.fillRect(0, 0, w, h);

    const dpr = window.devicePixelRatio || 1;
    const legendWidth = 40 * dpr; // Reserve space for legend on left
    const statsWidth = 140 * dpr; // Reserve space for stats on right
    const waveformLeft = legendWidth;
    const waveformWidth = w - legendWidth - statsWidth;
    const waveformRight = waveformLeft + waveformWidth;

    const voltageRange = maxVoltage - minVoltage;
    const pixelsPerVolt = h / voltageRange;
    const zeroY = h - (0 - minVoltage) * pixelsPerVolt;

    // Draw reference lines
    ctx.strokeStyle = borderColor;
    ctx.lineWidth = 1;
    ctx.setLineDash([]);

    // Center line (0V) - solid (if 0V is within the range)
    if (minVoltage <= 0 && maxVoltage >= 0) {
        ctx.beginPath();
        ctx.moveTo(waveformLeft, zeroY);
        ctx.lineTo(waveformRight, zeroY);
        ctx.stroke();
    }

    // +maxVoltage and minVoltage lines - dashed
    ctx.setLineDash([4 * dpr, 4 * dpr]);
    const topY = 0;
    const bottomY = h;

    ctx.beginPath();
    ctx.moveTo(waveformLeft, topY);
    ctx.lineTo(waveformRight, topY);
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(waveformLeft, bottomY);
    ctx.lineTo(waveformRight, bottomY);
    ctx.stroke();

    ctx.setLineDash([]);

    // Draw legend on left
    ctx.fillStyle = mutedColor;
    ctx.font = `${10 * dpr}px "Fira Code", monospace`;
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';

    const legendX = legendWidth - 4 * dpr;
    const textVerticalOffset = 10 * dpr;
    ctx.fillText(
        `${maxVoltage.toFixed(1)}v`,
        legendX,
        topY + textVerticalOffset,
    );
    if (minVoltage <= 0 && maxVoltage >= 0) {
        ctx.fillText('0v', legendX, zeroY);
    }
    ctx.fillText(
        `${minVoltage.toFixed(1)}v`,
        legendX,
        bottomY - textVerticalOffset,
    );

    // Draw stats on right
    if (stats) {
        ctx.textAlign = 'left';
        const statsX = waveformRight + 8 * dpr;
        const lineHeight = 14 * dpr;

        ctx.fillText(
            `min: ${stats.min.toFixed(2)}v`,
            statsX,
            h / 2 - lineHeight,
        );
        ctx.fillText(`max: ${stats.max.toFixed(2)}v`, statsX, h / 2);
        ctx.fillText(
            `p-p: ${stats.peakToPeak.toFixed(2)}v`,
            statsX,
            h / 2 + lineHeight,
        );
    }

    // Handle empty data
    if (
        !channels ||
        channels.length === 0 ||
        channels.every((ch) => ch.length === 0)
    ) {
        ctx.fillStyle = mutedColor;
        ctx.font = `${13 * dpr}px "Fira Code", monospace`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText('~', waveformLeft + waveformWidth / 2, zeroY);
        return;
    }

    const windowSize = 1024;

    // Draw all channels (same color, overlaid)
    ctx.strokeStyle = accentColor;
    ctx.lineWidth = 1.5 * dpr;

    for (const [ch, data] of channels.entries()) {
        if (!data || data.length < 2) {
            continue;
        }

        const sampleCount = Math.min(windowSize, data.length);
        const stepX = waveformWidth / (windowSize - 1);

        ctx.beginPath();

        for (let i = 0; i < sampleCount; i++) {
            const dataIndex = (i + stats.readOffset[ch]) % data.length;
            const x = waveformLeft + stepX * i;
            const rawSample = data[dataIndex];
            const clampedSample = Math.max(
                minVoltage,
                Math.min(maxVoltage, rawSample),
            );
            const y = h - (clampedSample - minVoltage) * pixelsPerVolt;

            if (i === 0) {
                ctx.moveTo(x, y);
            } else {
                ctx.lineTo(x, y);
            }
        }

        ctx.stroke();
    }
};
