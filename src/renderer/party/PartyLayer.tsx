import { useEffect, useRef, useState } from 'react';
import { CowboyScene, WESTERN_FONT } from './cowboy';
import { CramerScene } from './cramer';
import { CRITTERS, drawCowboyHat, drawFlyingGull, type Groove } from './critters';
import { partyLevels } from './partyLevels';
import { setSfxEnabled } from './sfx';
import './party.css';

type Mode = 'off' | 'cramer' | 'cowboy';

interface Settings {
    critters: boolean;
    mode: Mode;
    sfx: boolean;
}

const STORAGE_KEY = 'operator.party';
const DEFAULTS: Settings = { critters: true, mode: 'off', sfx: true };

function loadSettings(): Settings {
    try {
        const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}') as Partial<Settings>;
        return { ...DEFAULTS, ...stored };
    } catch {
        return DEFAULTS;
    }
}

/** How tall a critter is drawn, in CSS pixels per unit. */
const CRITTER_SCALE = 1.05;
/** How long a quip's speech bubble stays up. */
const QUIP_MS = 1600;
/** Beat strength that sets off a special move. */
const SPECIAL_STRENGTH = 0.8;

interface CritterState {
    special: number;
    quip: string;
    quipUntil: number;
}

/**
 * The party layer over the editor: critters dancing to the music along the
 * bottom, and the Cramer and cowboy modes, each a scene behind the code and
 * effects in front of it. Its toolbar sits in the editor's top-right corner.
 */
export function PartyLayer() {
    const [settings, setSettings] = useState(loadSettings);
    const backRef = useRef<HTMLCanvasElement>(null);
    const frontRef = useRef<HTMLCanvasElement>(null);
    const { critters, mode, sfx } = settings;

    useEffect(() => {
        try {
            localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
        } catch {
            // Settings just won't survive a reload.
        }
    }, [settings]);

    useEffect(() => setSfxEnabled(sfx), [sfx]);

    useEffect(() => {
        partyLevels.wanted = critters || mode !== 'off';
        const panel = backRef.current?.parentElement;
        panel?.classList.toggle('has-party-backdrop', mode !== 'off');
        panel?.classList.toggle('party-cowboy', mode === 'cowboy');
        return () => {
            partyLevels.wanted = false;
            panel?.classList.remove('has-party-backdrop', 'party-cowboy');
        };
    }, [critters, mode]);

    useEffect(() => {
        const back = backRef.current;
        const front = frontRef.current;
        if (back === null || front === null) return;
        const backCtx = back.getContext('2d')!;
        const frontCtx = front.getContext('2d')!;
        const cramer = mode === 'cramer' ? new CramerScene() : null;
        const cowboy = mode === 'cowboy' ? new CowboyScene() : null;
        const states: CritterState[] = CRITTERS.map(() => ({ quip: '', quipUntil: 0, special: 0 }));
        const stopBeat = partyLevels.onBeat((strength) => {
            if (strength > SPECIAL_STRENGTH) {
                for (const s of states) if (Math.random() < 0.5) s.special = 0.6;
            }
            if (Math.random() < 0.18) {
                const i = Math.floor(Math.random() * CRITTERS.length);
                const quips = mode === 'cowboy' ? ['Yeehaw!', 'Howdy, partner', ...CRITTERS[i].quips] : CRITTERS[i].quips;
                states[i].quip = quips[Math.floor(Math.random() * quips.length)];
                states[i].quipUntil = performance.now() + QUIP_MS;
            }
        });

        const start = performance.now();
        let last = start;
        let frame = 0;
        const draw = () => {
            frame = requestAnimationFrame(draw);
            const now = performance.now();
            const dt = Math.min(0.1, (now - last) / 1000);
            last = now;
            const t = (now - start) / 1000;
            partyLevels.tick(dt);

            const dpr = window.devicePixelRatio || 1;
            const w = front.clientWidth;
            const h = front.clientHeight;
            for (const canvas of [back, front]) {
                if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
                    canvas.width = Math.round(w * dpr);
                    canvas.height = Math.round(h * dpr);
                }
            }
            backCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
            frontCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
            backCtx.clearRect(0, 0, w, h);
            frontCtx.clearRect(0, 0, w, h);

            const overVideo = back.parentElement?.classList.contains('has-video-backdrop') ?? false;
            cramer?.drawBack(backCtx, w, h, t);
            cowboy?.drawBack(backCtx, w, h, t, overVideo);
            const groundY = h - 10;
            cowboy?.drawFront(frontCtx, w, h, t, dt, groundY);
            if (critters) drawCritters(frontCtx, w, groundY, t, dt, states, mode === 'cowboy');
            cramer?.drawFront(frontCtx, w, h, t);
        };
        draw();
        return () => {
            cancelAnimationFrame(frame);
            stopBeat();
            cramer?.dispose();
            cowboy?.dispose();
        };
    }, [critters, mode]);

    const setMode = (next: Mode) => setSettings((s) => ({ ...s, mode: s.mode === next ? 'off' : next }));

    return (
        <>
            <canvas ref={backRef} className="party-back" />
            <canvas ref={frontRef} className="party-front" />
            <div className="party-toolbar">
                <button
                    className={critters ? 'on' : ''}
                    title="Critters that dance to the music"
                    onClick={() => setSettings((s) => ({ ...s, critters: !s.critters }))}
                >
                    🐨 Critters
                </button>
                <button
                    className={mode === 'cramer' ? 'on' : ''}
                    title="Cramer mode: BOOYAH"
                    onClick={() => setMode('cramer')}
                >
                    📈 Cramer
                </button>
                <button
                    className={mode === 'cowboy' ? 'on' : ''}
                    title="Cowboy mode"
                    onClick={() => setMode('cowboy')}
                    style={{ fontFamily: WESTERN_FONT }}
                >
                    🤠 Cowboy
                </button>
                <button
                    className={sfx ? 'on' : ''}
                    title="Sound effects"
                    onClick={() => setSettings((s) => ({ ...s, sfx: !s.sfx }))}
                >
                    {sfx ? '🔊' : '🔇'}
                </button>
            </div>
        </>
    );
}

function drawCritters(
    ctx: CanvasRenderingContext2D,
    w: number,
    groundY: number,
    t: number,
    dt: number,
    states: CritterState[],
    hats: boolean,
): void {
    const { level, pulse } = partyLevels;
    const totalUnits = CRITTERS.reduce((sum, c) => sum + c.width, 0);
    const scale = Math.min(CRITTER_SCALE, (w * 0.92) / totalUnits);
    const spacing = w / CRITTERS.length;

    // Gulls wheeling overhead.
    for (let i = 0; i < 2; i++) {
        const x = ((t * (40 + i * 25) + i * w * 0.5) % (w + 120)) - 60;
        const y = groundY - 170 * scale + Math.sin(t * 1.3 + i * 2) * 18;
        ctx.save();
        ctx.translate(x, y);
        ctx.scale(scale * 0.8, scale * 0.8);
        drawFlyingGull(ctx, Math.sin(t * (5 + level * 10) + i));
        ctx.restore();
    }

    CRITTERS.forEach((critter, i) => {
        const state = states[i];
        state.special = Math.max(0, state.special - dt);
        const phase = i * 1.37;
        const groove: Groove = { level, phase, pulse, special: state.special, t };
        const hop = pulse * 14 + Math.abs(Math.sin(t * 4 + phase)) * level * 10;
        const dash = state.special > 0 ? Math.sin((1 - state.special / 0.6) * Math.PI) * 50 : 0;
        const sway = Math.sin(t * 3 + phase) * (0.04 + level * 0.12);
        const squash = pulse * 0.08;
        const x = spacing * (i + 0.5) + dash;

        ctx.save();
        ctx.translate(x, groundY);
        ctx.fillStyle = 'rgba(0,0,0,0.3)';
        ctx.beginPath();
        ctx.ellipse(0, 0, critter.width * 0.4 * scale * (1 - hop / 60), 5 * scale, 0, 0, Math.PI * 2);
        ctx.fill();
        ctx.translate(0, -hop);
        ctx.scale(scale * (1 + squash), scale * (1 - squash));
        ctx.rotate(sway);
        ctx.lineWidth = 2;
        ctx.strokeStyle = '#111';
        ctx.lineJoin = 'round';
        critter.draw(ctx, groove);
        if (hats && !(critter.name === 'Sonic' && state.special > 0)) {
            ctx.save();
            ctx.translate(critter.hat.x, critter.hat.y);
            drawCowboyHat(ctx, critter.hat.scale, (critter.hat.tilt ?? 0) + Math.sin(t * 6 + phase) * 0.08);
            ctx.restore();
        }
        ctx.restore();

        if (performance.now() < state.quipUntil) {
            drawBubble(ctx, x, groundY - hop + (critter.hat.y - 18) * scale, state.quip, hats);
        }
    });
}

function drawBubble(ctx: CanvasRenderingContext2D, x: number, y: number, text: string, western: boolean): void {
    ctx.save();
    ctx.font = western ? `bold 14px ${WESTERN_FONT}` : 'bold 14px "Comic Sans MS", "Chalkboard SE", sans-serif';
    const tw = ctx.measureText(text).width;
    const bx = Math.max(6, Math.min(x - tw / 2 - 10, ctx.canvas.clientWidth - tw - 26));
    const by = y - 34;
    ctx.fillStyle = '#fff';
    ctx.strokeStyle = '#111';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.roundRect(bx, by, tw + 20, 26, 12);
    ctx.moveTo(x - 6, by + 26);
    ctx.lineTo(x, by + 36);
    ctx.lineTo(x + 6, by + 26);
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = '#111';
    ctx.textBaseline = 'middle';
    ctx.fillText(text, bx + 10, by + 13);
    ctx.restore();
}
