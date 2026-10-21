/**
 * Sound-board effects, synthesized in Web Audio so no samples ship: a bull,
 * a cash register, a buzzer, a falling bomb, an air horn, a flush, a
 * "booyah", and for cowboy mode a whip crack, spurs and a "yeehaw". They play
 * in the renderer, beside the engine's own output.
 */

let context: AudioContext | null = null;
let master: GainNode | null = null;
let enabled = true;

function audio(): { ctx: AudioContext; out: GainNode } | null {
    if (!enabled) return null;
    if (context === null) {
        context = new AudioContext();
        master = context.createGain();
        master.gain.value = 0.32;
        const limiter = context.createDynamicsCompressor();
        limiter.threshold.value = -10;
        limiter.ratio.value = 12;
        master.connect(limiter).connect(context.destination);
    }
    if (context.state === 'suspended') void context.resume();
    return { ctx: context, out: master! };
}

export function setSfxEnabled(on: boolean): void {
    enabled = on;
    if (!on) void context?.suspend();
}

function noiseBuffer(ctx: AudioContext, seconds: number): AudioBuffer {
    const buffer = ctx.createBuffer(1, Math.ceil(ctx.sampleRate * seconds), ctx.sampleRate);
    const data = buffer.getChannelData(0);
    for (let i = 0; i < data.length; i++) data[i] = Math.random() * 2 - 1;
    return buffer;
}

function envelope(
    ctx: AudioContext,
    out: AudioNode,
    at: number,
    attack: number,
    hold: number,
    release: number,
    peak = 1,
): GainNode {
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, at);
    g.gain.linearRampToValueAtTime(peak, at + attack);
    g.gain.setValueAtTime(peak, at + attack + hold);
    g.gain.exponentialRampToValueAtTime(0.0001, at + attack + hold + release);
    g.connect(out);
    return g;
}

function osc(
    ctx: AudioContext,
    type: OscillatorType,
    freq: number,
    at: number,
    seconds: number,
    dest: AudioNode,
): OscillatorNode {
    const o = ctx.createOscillator();
    o.type = type;
    o.frequency.setValueAtTime(freq, at);
    o.connect(dest);
    o.start(at);
    o.stop(at + seconds + 0.05);
    return o;
}

function noise(ctx: AudioContext, at: number, seconds: number, dest: AudioNode): AudioBufferSourceNode {
    const n = ctx.createBufferSource();
    n.buffer = noiseBuffer(ctx, seconds + 0.1);
    n.connect(dest);
    n.start(at);
    return n;
}

function filter(ctx: AudioContext, type: BiquadFilterType, freq: number, q: number, dest: AudioNode): BiquadFilterNode {
    const f = ctx.createBiquadFilter();
    f.type = type;
    f.frequency.value = freq;
    f.Q.value = q;
    f.connect(dest);
    return f;
}

/** A vowel glide through two formant pairs, for the shouted words. */
function shout(
    vowels: [number, number][],
    pitches: number[],
    seconds: number,
    loud = 1,
): void {
    const a = audio();
    if (a === null) return;
    const { ctx, out } = a;
    const t = ctx.currentTime;
    const env = envelope(ctx, out, t, 0.02, seconds * 0.7, seconds * 0.3, 0.9 * loud);
    const formants = [0, 1].map((i) => {
        const f = filter(ctx, 'bandpass', vowels[0][i], 9, env);
        vowels.forEach((v, k) =>
            f.frequency.linearRampToValueAtTime(v[i], t + (seconds * k) / Math.max(1, vowels.length - 1)),
        );
        return f;
    });
    const source = osc(ctx, 'sawtooth', pitches[0], t, seconds, formants[0]);
    source.connect(formants[1]);
    pitches.forEach((p, k) =>
        source.frequency.linearRampToValueAtTime(p, t + (seconds * k) / Math.max(1, pitches.length - 1)),
    );
    const vibrato = osc(ctx, 'sine', 6, t, seconds, ctx.createGain());
    const depth = ctx.createGain();
    depth.gain.value = pitches[0] * 0.03;
    vibrato.disconnect();
    vibrato.connect(depth).connect(source.frequency);
}

export const sfx = {
    booyah(): void {
        // "boo" (u) gliding to "yah" (a), pitch leaping up.
        shout(
            [
                [320, 800],
                [300, 870],
                [280, 2250],
                [750, 1250],
                [800, 1200],
            ],
            [170, 160, 260, 300, 240],
            0.9,
            1.2,
        );
    },

    bull(): void {
        const a = audio();
        if (a === null) return;
        const { ctx, out } = a;
        const t = ctx.currentTime;
        const env = envelope(ctx, out, t, 0.08, 0.7, 0.5, 0.8);
        const lp = filter(ctx, 'lowpass', 700, 4, env);
        lp.frequency.linearRampToValueAtTime(1300, t + 0.3);
        lp.frequency.linearRampToValueAtTime(400, t + 1.2);
        for (const detune of [0, 7, -5]) {
            const o = osc(ctx, 'sawtooth', 110 + detune, t, 1.3, lp);
            o.frequency.linearRampToValueAtTime(150 + detune, t + 0.25);
            o.frequency.exponentialRampToValueAtTime(70 + detune, t + 1.2);
        }
        noise(ctx, t, 1.2, filter(ctx, 'bandpass', 500, 1, envelope(ctx, out, t, 0.05, 0.6, 0.5, 0.25)));
    },

    kaching(): void {
        const a = audio();
        if (a === null) return;
        const { ctx, out } = a;
        const t = ctx.currentTime;
        // The drawer's clunk, then the bell.
        noise(ctx, t, 0.08, filter(ctx, 'bandpass', 1800, 2, envelope(ctx, out, t, 0.001, 0.02, 0.06, 0.8)));
        noise(ctx, t + 0.09, 0.1, filter(ctx, 'lowpass', 600, 1, envelope(ctx, out, t + 0.09, 0.001, 0.02, 0.08, 0.9)));
        for (const [f, g] of [
            [2093, 0.5],
            [3136, 0.35],
            [4186, 0.25],
            [5274, 0.15],
        ]) {
            osc(ctx, 'sine', f, t + 0.16, 1.4, envelope(ctx, out, t + 0.16, 0.002, 0.05, 1.3, g));
        }
    },

    buzzer(): void {
        const a = audio();
        if (a === null) return;
        const { ctx, out } = a;
        const t = ctx.currentTime;
        const env = envelope(ctx, out, t, 0.01, 0.6, 0.08, 0.5);
        const lp = filter(ctx, 'lowpass', 1800, 1, env);
        osc(ctx, 'square', 98, t, 0.7, lp);
        osc(ctx, 'sawtooth', 104, t, 0.7, lp);
    },

    bomb(): void {
        const a = audio();
        if (a === null) return;
        const { ctx, out } = a;
        const t = ctx.currentTime;
        const whistle = osc(ctx, 'sine', 2400, t, 1.3, envelope(ctx, out, t, 0.05, 1.0, 0.2, 0.35));
        whistle.frequency.exponentialRampToValueAtTime(350, t + 1.25);
        const boomAt = t + 1.3;
        const lp = filter(ctx, 'lowpass', 900, 0.7, envelope(ctx, out, boomAt, 0.005, 0.15, 1.6, 1.2));
        lp.frequency.exponentialRampToValueAtTime(80, boomAt + 1.6);
        noise(ctx, boomAt, 1.8, lp);
        const sub = osc(ctx, 'sine', 90, boomAt, 1.2, envelope(ctx, out, boomAt, 0.005, 0.1, 1.0, 1));
        sub.frequency.exponentialRampToValueAtTime(30, boomAt + 1);
    },

    airhorn(): void {
        const a = audio();
        if (a === null) return;
        const { ctx, out } = a;
        const t = ctx.currentTime;
        [0, 0.28, 0.56].forEach((offset, i) => {
            const len = i === 2 ? 0.9 : 0.2;
            const env = envelope(ctx, out, t + offset, 0.01, len, 0.12, 0.45);
            const bp = filter(ctx, 'peaking', 1400, 1, env);
            bp.gain.value = 8;
            for (const f of [466, 470, 587, 700]) osc(ctx, 'sawtooth', f, t + offset, len + 0.12, bp);
        });
    },

    flush(): void {
        const a = audio();
        if (a === null) return;
        const { ctx, out } = a;
        const t = ctx.currentTime;
        const env = envelope(ctx, out, t, 0.3, 1.0, 1.0, 0.7);
        const bp = filter(ctx, 'bandpass', 900, 2, env);
        const wobble = osc(ctx, 'sine', 5, t, 2.4, ctx.createGain());
        const depth = ctx.createGain();
        depth.gain.value = 500;
        wobble.disconnect();
        wobble.connect(depth).connect(bp.frequency);
        bp.frequency.linearRampToValueAtTime(400, t + 2.2);
        noise(ctx, t, 2.4, bp);
    },

    whip(): void {
        const a = audio();
        if (a === null) return;
        const { ctx, out } = a;
        const t = ctx.currentTime;
        const hp = filter(ctx, 'highpass', 1500, 0.7, envelope(ctx, out, t + 0.12, 0.001, 0.01, 0.12, 1.1));
        noise(ctx, t + 0.12, 0.2, hp);
        const swish = filter(ctx, 'bandpass', 400, 3, envelope(ctx, out, t, 0.08, 0.02, 0.04, 0.3));
        swish.frequency.exponentialRampToValueAtTime(4000, t + 0.13);
        noise(ctx, t, 0.15, swish);
    },

    spurs(): void {
        const a = audio();
        if (a === null) return;
        const { ctx, out } = a;
        const t = ctx.currentTime;
        for (let i = 0; i < 4; i++) {
            const at = t + i * 0.035 + Math.random() * 0.02;
            for (const f of [3520, 4700, 6100]) {
                osc(ctx, 'sine', f * (0.98 + Math.random() * 0.04), at, 0.4, envelope(ctx, out, at, 0.001, 0.01, 0.35, 0.08));
            }
        }
    },

    yeehaw(): void {
        // "yee" (i) up high, then "haw" (ɔ) sliding down.
        shout(
            [
                [280, 2300],
                [300, 2200],
                [600, 1000],
                [580, 900],
            ],
            [330, 420, 380, 220],
            0.85,
        );
    },
};
