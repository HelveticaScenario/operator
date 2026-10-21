/**
 * Cowboy mode: a desert sunset behind the code, the patch's video gone sepia,
 * tumbleweeds rolling to the beat, a rider on the ridge, and a whip crack
 * when the music hits hard.
 */
import { partyLevels } from './partyLevels';
import { sfx } from './sfx';

interface Tumbleweed {
    x: number;
    r: number;
    speed: number;
    spin: number;
    hop: number;
}

interface Dust {
    x: number;
    y: number;
    vx: number;
    vy: number;
    life: number;
}

const HOLLERS = ['YEEHAW!', 'HOWDY!', 'GIDDY UP!', 'RIDE ’EM!', 'YEE-HAW!!', 'HOO-WEE!', 'SADDLE UP'];

export const WESTERN_FONT = '"Rye", "Rockwell Extra Bold", Rockwell, "Copperplate", "American Typewriter", Georgia, serif';

export class CowboyScene {
    private readonly weeds: Tumbleweed[] = [];
    private readonly dust: Dust[] = [];
    private holler = '';
    private hollerMs = -1e9;
    private riderStart = 0;
    private readonly stopBeat: () => void;

    constructor() {
        for (let i = 0; i < 3; i++) this.spawnWeed(Math.random());
        this.riderStart = performance.now();
        this.stopBeat = partyLevels.onBeat((strength) => {
            const beat = partyLevels.beats;
            for (const weed of this.weeds) weed.hop = Math.max(weed.hop, strength);
            if (beat % 16 === 0) {
                this.shout();
            } else if (strength > 0.75 && beat % 4 === 0) {
                sfx.whip();
            } else if (beat % 4 === 2) {
                sfx.spurs();
            }
            if (Math.random() < 0.15 && this.weeds.length < 6) this.spawnWeed(-0.1);
        });
        this.shout();
    }

    dispose(): void {
        this.stopBeat();
    }

    private shout(): void {
        this.holler = HOLLERS[Math.floor(Math.random() * HOLLERS.length)];
        this.hollerMs = performance.now();
        sfx.yeehaw();
        setTimeout(() => sfx.whip(), 700);
    }

    private spawnWeed(x: number): void {
        this.weeds.push({ hop: 0, r: 14 + Math.random() * 16, speed: 0.04 + Math.random() * 0.05, spin: 0, x });
    }

    /** Sky, sun, mesas and cacti. Thinner over the patch's own video. */
    drawBack(ctx: CanvasRenderingContext2D, w: number, h: number, t: number, overVideo: boolean): void {
        const { level, pulse } = partyLevels;
        ctx.save();
        ctx.globalAlpha = overVideo ? 0.55 : 1;
        const sky = ctx.createLinearGradient(0, 0, 0, h);
        sky.addColorStop(0, '#2b1a4a');
        sky.addColorStop(0.45, '#c2410c');
        sky.addColorStop(0.7, '#f59e0b');
        sky.addColorStop(1, '#fde68a');
        ctx.fillStyle = sky;
        ctx.fillRect(0, 0, w, h);

        const sunR = Math.min(w, h) * (0.14 + level * 0.05 + pulse * 0.02);
        const sunY = h * 0.62;
        const glow = ctx.createRadialGradient(w / 2, sunY, sunR * 0.5, w / 2, sunY, sunR * 3);
        glow.addColorStop(0, 'rgba(255,240,180,0.8)');
        glow.addColorStop(1, 'rgba(255,240,180,0)');
        ctx.fillStyle = glow;
        ctx.fillRect(0, 0, w, h);
        ctx.fillStyle = '#fff3c4';
        ctx.beginPath();
        ctx.arc(w / 2, sunY, sunR, 0, Math.PI * 2);
        ctx.fill();
        // Heat-shimmer bands across the sun.
        ctx.fillStyle = '#f59e0b';
        for (let i = 0; i < 5; i++) {
            const y = sunY + sunR * (0.15 + i * 0.18);
            ctx.fillRect(w / 2 - sunR, y + Math.sin(t * 3 + i) * 2, sunR * 2, 3 + i * 1.5);
        }

        // Mesas on the horizon.
        ctx.fillStyle = '#5b2a12';
        ctx.beginPath();
        ctx.moveTo(0, h * 0.75);
        const mesas = [0.08, 0.72, 0.18, 0.6, 0.32, 0.6, 0.4, 0.74, 0.62, 0.74, 0.7, 0.55, 0.88, 0.55, 0.95, 0.73, 1, 0.73];
        for (let i = 0; i < mesas.length; i += 2) ctx.lineTo(w * mesas[i], h * mesas[i + 1]);
        ctx.lineTo(w, h);
        ctx.lineTo(0, h);
        ctx.fill();
        ctx.fillStyle = '#7c3a12';
        ctx.fillRect(0, h * 0.78, w, h * 0.22);

        this.drawRider(ctx, w, h, t);

        // Saguaros swaying with the loudness.
        for (const [fx, size] of [
            [0.12, 1],
            [0.85, 1.25],
            [0.6, 0.6],
        ]) {
            this.drawCactus(ctx, w * fx, h * 0.86, size * Math.min(w, h) * 0.0026, Math.sin(t * 2 + fx * 9) * (0.03 + level * 0.12));
        }
        ctx.restore();
    }

    private drawRider(ctx: CanvasRenderingContext2D, w: number, h: number, t: number): void {
        const lap = 14000;
        const k = ((performance.now() - this.riderStart) % lap) / lap;
        const x = -100 + k * (w + 200);
        const gallop = Math.sin(t * 14) * 4;
        const y = h * 0.6 - 8 + gallop;
        ctx.save();
        ctx.translate(x, y);
        ctx.fillStyle = '#1c0d05';
        // Horse.
        ctx.beginPath();
        ctx.ellipse(0, 0, 30, 11, 0, 0, Math.PI * 2);
        ctx.fill();
        ctx.beginPath();
        ctx.moveTo(22, -4);
        ctx.lineTo(40, -22);
        ctx.lineTo(48, -18);
        ctx.lineTo(30, 4);
        ctx.fill();
        ctx.lineWidth = 4;
        ctx.strokeStyle = '#1c0d05';
        for (const [lx, phase] of [
            [-20, 0],
            [-12, Math.PI],
            [14, Math.PI / 2],
            [22, (3 * Math.PI) / 2],
        ]) {
            const swing = Math.sin(t * 14 + phase) * 10;
            ctx.beginPath();
            ctx.moveTo(lx, 6);
            ctx.lineTo(lx + swing, 26);
            ctx.stroke();
        }
        ctx.beginPath();
        ctx.moveTo(-28, -4);
        ctx.quadraticCurveTo(-44, 0, -42, 16);
        ctx.stroke();
        // Rider, hat and a lasso overhead.
        ctx.fillRect(-4, -30, 9, 22);
        ctx.beginPath();
        ctx.arc(0, -36, 6, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillRect(-11, -42, 22, 3);
        ctx.fillRect(-5, -49, 10, 8);
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.ellipse(4, -64, 18, 5, Math.sin(t * 6) * 0.3, 0, Math.PI * 2);
        ctx.moveTo(4, -30);
        ctx.lineTo(4 + Math.cos(t * 6) * 18, -64);
        ctx.stroke();
        ctx.restore();
    }

    private drawCactus(ctx: CanvasRenderingContext2D, x: number, y: number, s: number, sway: number): void {
        ctx.save();
        ctx.translate(x, y);
        ctx.rotate(sway);
        ctx.scale(s, s);
        ctx.fillStyle = '#14532d';
        ctx.strokeStyle = '#052e16';
        ctx.lineWidth = 3;
        const rounded = (rx: number, ry: number, rw: number, rh: number) => {
            ctx.beginPath();
            ctx.roundRect(rx, ry, rw, rh, rw / 2);
            ctx.fill();
            ctx.stroke();
        };
        rounded(-14, -140, 28, 140);
        rounded(-48, -100, 22, 55);
        rounded(-48, -58, 40, 18);
        rounded(26, -120, 22, 60);
        rounded(8, -72, 40, 18);
        ctx.restore();
    }

    /** Tumbleweeds, dust and the holler, over the code. */
    drawFront(ctx: CanvasRenderingContext2D, w: number, h: number, t: number, dt: number, groundY: number): void {
        const { level, pulse } = partyLevels;
        for (const weed of this.weeds) {
            weed.x += weed.speed * dt * (1 + level * 4);
            weed.spin += (weed.speed * dt * (1 + level * 4) * w) / weed.r;
            weed.hop *= Math.exp(-dt * 3);
            const bounce = Math.abs(Math.sin(weed.x * 40)) * 10 + weed.hop * 40;
            const x = weed.x * w;
            const y = groundY - weed.r - bounce;
            if (Math.random() < 0.3 + level) {
                this.dust.push({ life: 1, vx: -20 - Math.random() * 30, vy: -10 - Math.random() * 20, x, y: groundY - 4 });
            }
            ctx.save();
            ctx.translate(x, y);
            ctx.rotate(weed.spin);
            ctx.strokeStyle = '#a16207';
            ctx.lineWidth = 1.5;
            for (let i = 0; i < 9; i++) {
                ctx.beginPath();
                ctx.ellipse(0, 0, weed.r, weed.r * 0.45, (i * Math.PI) / 9, 0, Math.PI * 2);
                ctx.stroke();
            }
            ctx.restore();
        }
        for (let i = this.weeds.length - 1; i >= 0; i--) {
            if (this.weeds[i].x > 1.15) {
                this.weeds.splice(i, 1);
                this.spawnWeed(-0.15);
            }
        }
        for (const d of this.dust) {
            d.x += d.vx * dt;
            d.y += d.vy * dt;
            d.life -= dt * 1.2;
            ctx.fillStyle = `rgba(214,170,110,${Math.max(0, d.life) * 0.5})`;
            ctx.beginPath();
            ctx.arc(d.x, d.y, 3 + (1 - d.life) * 6, 0, Math.PI * 2);
            ctx.fill();
        }
        for (let i = this.dust.length - 1; i >= 0; i--) if (this.dust[i].life <= 0) this.dust.splice(i, 1);

        // The holler on a swinging wooden sign.
        const age = (performance.now() - this.hollerMs) / 1000;
        if (age < 2.6) {
            const fade = age < 2 ? 1 : 1 - (age - 2) / 0.6;
            const size = Math.min(w / 7, 140);
            ctx.save();
            ctx.globalAlpha = fade;
            ctx.translate(w / 2, h * 0.32);
            ctx.rotate(Math.sin(age * 6) * 0.08 * Math.exp(-age));
            ctx.scale(1 + pulse * 0.06, 1 + pulse * 0.06);
            ctx.font = `bold ${size}px ${WESTERN_FONT}`;
            const tw = ctx.measureText(this.holler).width;
            ctx.fillStyle = '#7c4a1e';
            ctx.strokeStyle = '#3b1f0b';
            ctx.lineWidth = 8;
            ctx.beginPath();
            ctx.roundRect(-tw / 2 - 40, -size * 0.75, tw + 80, size * 1.5, 16);
            ctx.fill();
            ctx.stroke();
            ctx.strokeStyle = 'rgba(59,31,11,0.4)';
            ctx.lineWidth = 2;
            for (let i = 1; i < 4; i++) {
                ctx.beginPath();
                ctx.moveTo(-tw / 2 - 36, -size * 0.75 + (size * 1.5 * i) / 4);
                ctx.lineTo(tw / 2 + 36, -size * 0.75 + (size * 1.5 * i) / 4);
                ctx.stroke();
            }
            ctx.textAlign = 'center';
            ctx.textBaseline = 'middle';
            ctx.lineWidth = size * 0.1;
            ctx.strokeStyle = '#2a1405';
            ctx.strokeText(this.holler, 0, size * 0.05);
            ctx.fillStyle = '#fde68a';
            ctx.fillText(this.holler, 0, size * 0.05);
            ctx.restore();
        }
        void t;
    }
}
