/**
 * Cramer mode: a money-show studio behind the code, the camera cutting and
 * morphing between punched-in shots of the host, his catchphrases in huge
 * letters, and the sound board going off with every call.
 *
 * Photos dropped into `src/renderer/assets/cramer/` are used for the shots;
 * without any, a drawn caricature of the host stands in.
 */
import { partyLevels } from './partyLevels';
import { sfx } from './sfx';

type Effect = keyof typeof sfx;

interface Call {
    text: string;
    color: string;
    effects: Effect[];
    /** Which way the chart behind the host goes. */
    trend: number;
}

const CALLS: Call[] = [
    { text: 'BUY! BUY! BUY!', color: '#39ff6a', effects: ['bull', 'kaching'], trend: 1 },
    { text: 'BOOYAH!', color: '#ffe23a', effects: ['booyah', 'airhorn'], trend: 0.6 },
    { text: 'SELL! SELL! SELL!', color: '#ff3b3b', effects: ['buzzer', 'flush'], trend: -1 },
    { text: "THERE'S ALWAYS A BULL MARKET SOMEWHERE", color: '#39ff6a', effects: ['bull'], trend: 0.8 },
    { text: 'ARE YOU KIDDING ME?!', color: '#ff9b2f', effects: ['bomb'], trend: -0.7 },
    { text: 'BULLS MAKE MONEY, BEARS MAKE MONEY, PIGS GET SLAUGHTERED', color: '#ff6bd5', effects: ['buzzer'], trend: -0.3 },
    { text: 'DO YOUR HOMEWORK!', color: '#6bd7ff', effects: ['kaching'], trend: 0.2 },
    { text: 'BUY THE DIP!', color: '#39ff6a', effects: ['kaching', 'airhorn'], trend: 1 },
    { text: 'BOOM-SHAKALAKA!', color: '#ffe23a', effects: ['bomb'], trend: 0.5 },
    { text: 'BOOYAH!!!', color: '#ffe23a', effects: ['booyah', 'kaching'], trend: 0.7 },
];

const TICKERS = ['BOOYA', 'MOO', 'KOALA', 'GULL', 'EDDY', 'JAWB', 'SONIC', 'YEEHA', 'BULL', 'BEAR', 'PIG', 'TOAST'];

/** Seconds a call stays up when no beats arrive. */
const CALL_SECONDS = 3.5;
/** Beats a call stays up while music plays. */
const CALL_BEATS = 8;

interface Shot {
    /** Point the camera centres on, in the source's 0–1 coordinates. */
    fx: number;
    fy: number;
    zoom: number;
    roll: number;
}

const photoUrls = Object.values(
    import.meta.glob('../assets/cramer/*.{png,jpg,jpeg,webp,gif,avif,PNG,JPG,JPEG,WEBP}', {
        eager: true,
        query: '?url',
        import: 'default',
    }),
);

/** Close-ups of the drawn host: face, glasses, mouth, fists, tie, the lot. */
const DRAWN_SHOTS: Shot[] = [
    { fx: 0.5, fy: 0.5, zoom: 1, roll: 0 },
    { fx: 0.5, fy: 0.3, zoom: 2.2, roll: -0.05 },
    { fx: 0.5, fy: 0.27, zoom: 4.2, roll: 0.08 },
    { fx: 0.5, fy: 0.38, zoom: 3.6, roll: -0.1 },
    { fx: 0.2, fy: 0.16, zoom: 3.2, roll: 0.12 },
    { fx: 0.8, fy: 0.16, zoom: 3.2, roll: -0.12 },
    { fx: 0.5, fy: 0.62, zoom: 2.6, roll: 0.04 },
    { fx: 0.42, fy: 0.27, zoom: 6, roll: 0 },
];

function randomShot(): Shot {
    return {
        fx: 0.35 + Math.random() * 0.3,
        fy: 0.25 + Math.random() * 0.3,
        zoom: 1.3 + Math.random() * 1.8,
        roll: (Math.random() - 0.5) * 0.2,
    };
}

const ease = (x: number) => (x < 0.5 ? 4 * x * x * x : 1 - (-2 * x + 2) ** 3 / 2);

export class CramerScene {
    private readonly photos: HTMLImageElement[] = [];
    private readonly host = document.createElement('canvas');
    private callIndex = -1;
    private callStartMs = 0;
    private callStartBeat = 0;
    private shotIndex = 0;
    private shotFrom: Shot = DRAWN_SHOTS[0];
    private shotTo: Shot = DRAWN_SHOTS[1];
    private photoFrom = 0;
    private photoTo = 0;
    private morphStartMs = 0;
    private chart: number[] = [];
    private trend = 0;
    private flash = 0;
    private readonly stopBeat: () => void;

    constructor() {
        for (const url of photoUrls) {
            const img = new Image();
            img.src = url;
            this.photos.push(img);
        }
        this.host.width = 900;
        this.host.height = 900;
        for (let i = 0; i < 120; i++) this.chart.push(0.5);
        this.stopBeat = partyLevels.onBeat((strength) => {
            this.flash = Math.max(this.flash, strength);
            if (partyLevels.beats % 2 === 0) this.nextShot();
        });
        this.nextCall();
    }

    dispose(): void {
        this.stopBeat();
    }

    get hasPhotos(): boolean {
        return this.photos.length > 0;
    }

    private nextCall(): void {
        let next = Math.floor(Math.random() * CALLS.length);
        if (next === this.callIndex) next = (next + 1) % CALLS.length;
        this.callIndex = next;
        this.callStartMs = performance.now();
        this.callStartBeat = partyLevels.beats;
        const call = CALLS[next];
        this.trend = call.trend;
        call.effects.forEach((effect, i) => {
            setTimeout(() => sfx[effect](), i * 450);
        });
        this.nextShot();
    }

    private nextShot(): void {
        const now = performance.now();
        this.shotFrom = this.currentShot(now);
        this.photoFrom = this.photoTo;
        if (this.photos.length > 0) {
            this.photoTo = (this.photoTo + 1 + Math.floor(Math.random() * Math.max(1, this.photos.length - 1))) % this.photos.length;
            this.shotTo = randomShot();
        } else {
            this.shotIndex = (this.shotIndex + 1 + Math.floor(Math.random() * (DRAWN_SHOTS.length - 1))) % DRAWN_SHOTS.length;
            this.shotTo = DRAWN_SHOTS[this.shotIndex];
        }
        this.morphStartMs = now;
    }

    private morph(now: number): number {
        return Math.min(1, (now - this.morphStartMs) / 650);
    }

    private currentShot(now: number): Shot {
        const k = ease(this.morph(now));
        const a = this.shotFrom;
        const b = this.shotTo;
        return {
            fx: a.fx + (b.fx - a.fx) * k,
            fy: a.fy + (b.fy - a.fy) * k,
            zoom: a.zoom * (b.zoom / a.zoom) ** k,
            roll: a.roll + (b.roll - a.roll) * k,
        };
    }

    /** The studio, the chart and the host, behind the code. */
    drawBack(ctx: CanvasRenderingContext2D, w: number, h: number, t: number): void {
        const now = performance.now();
        const { level, pulse } = partyLevels;
        const call = CALLS[this.callIndex];
        const beatsUp = partyLevels.beats - this.callStartBeat;
        if (now - this.callStartMs > CALL_SECONDS * 1000 && (beatsUp >= CALL_BEATS || level < 0.05)) {
            this.nextCall();
        }
        this.flash *= 0.9;

        // Studio: deep blue, spotlights, and a flash of the call's colour.
        const bg = ctx.createRadialGradient(w / 2, h * 0.4, 0, w / 2, h * 0.4, Math.max(w, h) * 0.8);
        bg.addColorStop(0, '#1b2a6b');
        bg.addColorStop(1, '#03040d');
        ctx.fillStyle = bg;
        ctx.fillRect(0, 0, w, h);
        ctx.save();
        ctx.globalCompositeOperation = 'lighter';
        for (let i = 0; i < 4; i++) {
            const sway = Math.sin(t * 0.7 + i * 1.7) * 0.25;
            const x = (w * (i + 0.5)) / 4;
            const grad = ctx.createLinearGradient(x, 0, x + sway * h, h);
            grad.addColorStop(0, `rgba(255,255,220,${0.12 + level * 0.15})`);
            grad.addColorStop(1, 'rgba(255,255,220,0)');
            ctx.fillStyle = grad;
            ctx.beginPath();
            ctx.moveTo(x - 10, 0);
            ctx.lineTo(x + 10, 0);
            ctx.lineTo(x + sway * h + 140, h);
            ctx.lineTo(x + sway * h - 140, h);
            ctx.fill();
        }
        ctx.globalAlpha = this.flash * 0.35;
        ctx.fillStyle = call.color;
        ctx.fillRect(0, 0, w, h);
        ctx.restore();

        this.drawChart(ctx, w, h);
        this.drawShots(ctx, w, h, t, now, pulse);

        // Scanlines and a vignette for the broadcast look.
        ctx.save();
        ctx.globalAlpha = 0.12;
        ctx.fillStyle = '#000';
        for (let y = 0; y < h; y += 4) ctx.fillRect(0, y, w, 2);
        ctx.restore();
        const vignette = ctx.createRadialGradient(w / 2, h / 2, Math.min(w, h) * 0.3, w / 2, h / 2, Math.max(w, h) * 0.75);
        vignette.addColorStop(0, 'rgba(0,0,0,0)');
        vignette.addColorStop(1, 'rgba(0,0,0,0.7)');
        ctx.fillStyle = vignette;
        ctx.fillRect(0, 0, w, h);
    }

    private drawChart(ctx: CanvasRenderingContext2D, w: number, h: number): void {
        const last = this.chart[this.chart.length - 1];
        const step = this.trend * 0.006 + (Math.random() - 0.5) * 0.025 * (1 + partyLevels.level * 2);
        this.chart.push(Math.min(0.95, Math.max(0.05, last + step)));
        this.chart.shift();
        const up = this.trend >= 0;
        ctx.save();
        ctx.lineWidth = 4;
        ctx.strokeStyle = up ? 'rgba(57,255,106,0.55)' : 'rgba(255,59,59,0.55)';
        ctx.shadowColor = ctx.strokeStyle;
        ctx.shadowBlur = 12;
        ctx.beginPath();
        this.chart.forEach((v, i) => {
            const x = (i / (this.chart.length - 1)) * w;
            const y = h * (1 - v);
            if (i === 0) ctx.moveTo(x, y);
            else ctx.lineTo(x, y);
        });
        ctx.stroke();
        ctx.restore();
    }

    private drawShots(ctx: CanvasRenderingContext2D, w: number, h: number, t: number, now: number, pulse: number): void {
        const k = this.morph(now);
        const shot = this.currentShot(now);
        // A punch-in on every beat, and a slow push the rest of the time.
        const push = 1 + pulse * 0.08 + ((now - this.morphStartMs) / 1000) * 0.03;
        if (this.photos.length > 0) {
            const from = this.photos[this.photoFrom];
            const to = this.photos[this.photoTo];
            if (k < 1) this.drawSource(ctx, from, w, h, this.shotFrom, push * (1 + k * 0.6), 1);
            // The incoming shot zoom-blurs in over the outgoing one.
            for (let i = 2; i >= 0; i--) {
                this.drawSource(ctx, to, w, h, shot, push * (1 + i * 0.05 * (1 - k)), k * (i === 0 ? 1 : 0.25));
            }
            return;
        }
        this.drawHost(t, pulse);
        if (k < 1) {
            for (let i = 2; i >= 1; i--) {
                this.drawSource(ctx, this.host, w, h, shot, push * (1 + i * 0.06 * (1 - k)), 0.3 * (1 - k));
            }
        }
        this.drawSource(ctx, this.host, w, h, shot, push, 1);
    }

    private drawSource(
        ctx: CanvasRenderingContext2D,
        source: HTMLImageElement | HTMLCanvasElement,
        w: number,
        h: number,
        shot: Shot,
        push: number,
        alpha: number,
    ): void {
        const sw = source instanceof HTMLImageElement ? source.naturalWidth : source.width;
        const sh = source instanceof HTMLImageElement ? source.naturalHeight : source.height;
        if (!sw || !sh || alpha <= 0) return;
        const cover = Math.max(w / sw, h / sh) * (source === this.host ? 0.85 : 1);
        const scale = cover * shot.zoom * push;
        ctx.save();
        ctx.globalAlpha = alpha;
        ctx.translate(w / 2, h / 2);
        ctx.rotate(shot.roll);
        ctx.scale(scale, scale);
        ctx.translate(-shot.fx * sw, -shot.fy * sh);
        ctx.drawImage(source, 0, 0);
        ctx.restore();
    }

    /** The drawn host, in a 900×900 frame: shouting, fists up, sleeves rolled. */
    private drawHost(t: number, pulse: number): void {
        const ctx = this.host.getContext('2d')!;
        const { level } = partyLevels;
        ctx.clearRect(0, 0, 900, 900);
        ctx.save();
        ctx.translate(450, 470);
        ctx.lineJoin = 'round';
        ctx.lineCap = 'round';
        ctx.lineWidth = 6;
        ctx.strokeStyle = '#1a0d08';
        const skin = '#f1b49a';
        const shout = 0.4 + level * 0.8 + pulse * 0.5;
        const shake = Math.sin(t * 30) * pulse * 6;

        // Shirt and loosened tie.
        ctx.beginPath();
        ctx.moveTo(-230, 430);
        ctx.quadraticCurveTo(-240, 120, -110, 80);
        ctx.lineTo(110, 80);
        ctx.quadraticCurveTo(240, 120, 230, 430);
        ctx.closePath();
        ctx.fillStyle = '#f4f6fb';
        ctx.fill();
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(-22, 95);
        ctx.lineTo(22, 95);
        ctx.lineTo(40, 300);
        ctx.lineTo(0, 345);
        ctx.lineTo(-40, 300);
        ctx.closePath();
        ctx.fillStyle = '#c3122f';
        ctx.fill();
        ctx.stroke();

        // Both fists up, pumping.
        for (const side of [-1, 1]) {
            const lift = Math.sin(t * 8 + (side > 0 ? 0 : Math.PI)) * 30 * (0.4 + level);
            const sx = side * 170;
            const hx = side * 330;
            const hy = -230 + lift;
            ctx.lineWidth = 70;
            ctx.strokeStyle = '#1a0d08';
            ctx.beginPath();
            ctx.moveTo(sx, 140);
            ctx.lineTo(side * 300, 10);
            ctx.lineTo(hx, hy);
            ctx.stroke();
            ctx.lineWidth = 60;
            ctx.strokeStyle = '#f4f6fb';
            ctx.beginPath();
            ctx.moveTo(sx, 140);
            ctx.lineTo(side * 300, 10);
            ctx.stroke();
            ctx.strokeStyle = skin;
            ctx.beginPath();
            ctx.moveTo(side * 300, 10);
            ctx.lineTo(hx, hy);
            ctx.stroke();
            ctx.lineWidth = 6;
            ctx.strokeStyle = '#1a0d08';
            ctx.beginPath();
            ctx.ellipse(hx, hy - 30, 52, 46, 0, 0, Math.PI * 2);
            ctx.fillStyle = skin;
            ctx.fill();
            ctx.stroke();
            for (let i = -1; i <= 1; i++) {
                ctx.beginPath();
                ctx.moveTo(hx + i * 18, hy - 70);
                ctx.lineTo(hx + i * 18, hy - 40);
                ctx.stroke();
            }
        }

        // The head: bald dome, grey fringe, glasses, wide-open shout.
        ctx.save();
        ctx.translate(shake, -110);
        ctx.rotate(Math.sin(t * 3) * 0.05);
        ctx.beginPath();
        ctx.ellipse(-150, 20, 26, 40, 0, 0, Math.PI * 2);
        ctx.ellipse(150, 20, 26, 40, 0, 0, Math.PI * 2);
        ctx.fillStyle = skin;
        ctx.fill();
        ctx.stroke();
        ctx.beginPath();
        ctx.ellipse(0, 0, 150, 185, 0, 0, Math.PI * 2);
        ctx.fillStyle = skin;
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = 'rgba(255,255,255,0.35)';
        ctx.beginPath();
        ctx.ellipse(-40, -120, 50, 25, -0.3, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = '#b9b4ad';
        for (const side of [-1, 1]) {
            ctx.beginPath();
            ctx.moveTo(side * 150, -40);
            ctx.quadraticCurveTo(side * 175, 20, side * 130, 90);
            ctx.lineTo(side * 118, 30);
            ctx.quadraticCurveTo(side * 135, -10, side * 150, -40);
            ctx.fill();
        }
        // Brows up, glasses on.
        ctx.lineWidth = 12;
        for (const side of [-1, 1]) {
            ctx.beginPath();
            ctx.moveTo(side * 25, -55 - shout * 25);
            ctx.lineTo(side * 105, -75 - shout * 20);
            ctx.stroke();
        }
        ctx.lineWidth = 5;
        for (const side of [-1, 1]) {
            ctx.beginPath();
            ctx.ellipse(side * 62, -15, 48, 36, 0, 0, Math.PI * 2);
            ctx.fillStyle = 'rgba(200,230,255,0.25)';
            ctx.fill();
            ctx.stroke();
            ctx.beginPath();
            ctx.arc(side * 62, -12, 13, 0, Math.PI * 2);
            ctx.fillStyle = '#2a1a10';
            ctx.fill();
        }
        ctx.beginPath();
        ctx.moveTo(-14, -18);
        ctx.quadraticCurveTo(0, -26, 14, -18);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(-10, 0);
        ctx.quadraticCurveTo(-26, 50, 0, 55);
        ctx.quadraticCurveTo(14, 55, 10, 45);
        ctx.stroke();
        ctx.beginPath();
        ctx.ellipse(0, 108, 70, 18 + shout * 45, 0, 0, Math.PI * 2);
        ctx.fillStyle = '#4a0d12';
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = '#fff';
        ctx.fillRect(-48, 108 - (18 + shout * 45) + 6, 96, 16);
        ctx.beginPath();
        ctx.ellipse(0, 108 + shout * 30, 34, 14, 0, 0, Math.PI * 2);
        ctx.fillStyle = '#e0505d';
        ctx.fill();
        ctx.restore();
        ctx.restore();
    }

    /** The call in giant letters, the ticker, and the fine print, over the code. */
    drawFront(ctx: CanvasRenderingContext2D, w: number, h: number, t: number): void {
        const call = CALLS[this.callIndex];
        const { pulse } = partyLevels;
        const age = (performance.now() - this.callStartMs) / 1000;
        const slam = age < 0.25 ? 1 + (0.25 - age) * 6 : 1;

        ctx.save();
        ctx.translate(w / 2 + Math.sin(t * 40) * pulse * 6, h * 0.42);
        ctx.rotate(Math.sin(t * 2) * 0.04 - 0.05);
        ctx.scale(slam + pulse * 0.08, slam + pulse * 0.08);
        const lines = wrap(call.text, 18);
        const size = Math.min(w / Math.max(...lines.map((l) => l.length)) * 1.6, h / (lines.length * 1.6), 180);
        ctx.font = `900 ${size}px Impact, "Arial Black", "Helvetica Neue", sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.lineJoin = 'round';
        lines.forEach((line, i) => {
            const y = (i - (lines.length - 1) / 2) * size * 1.05;
            ctx.lineWidth = size * 0.16;
            ctx.strokeStyle = '#000';
            ctx.strokeText(line, 0, y);
            ctx.fillStyle = call.color;
            ctx.shadowColor = call.color;
            ctx.shadowBlur = 20 + pulse * 30;
            ctx.fillText(line, 0, y);
            ctx.shadowBlur = 0;
        });
        ctx.restore();

        // The ticker crawl.
        const barH = 30;
        ctx.fillStyle = 'rgba(0,0,0,0.85)';
        ctx.fillRect(0, 0, w, barH);
        ctx.fillStyle = '#ffe23a';
        ctx.fillRect(0, barH, w, 3);
        ctx.font = 'bold 18px "Helvetica Neue", Arial, sans-serif';
        ctx.textBaseline = 'middle';
        ctx.textAlign = 'left';
        const itemW = 170;
        const offset = (t * 90) % (itemW * TICKERS.length);
        for (let i = -1; i < w / itemW + 2; i++) {
            const index = ((i + Math.floor(offset / itemW)) % TICKERS.length + TICKERS.length) % TICKERS.length;
            const x = i * itemW - (offset % itemW);
            const seed = Math.sin(index * 91.7 + Math.floor(t / 2) * 13.1);
            const up = seed + this.trend * 0.5 > 0;
            ctx.fillStyle = '#fff';
            ctx.fillText(TICKERS[index], x + 10, barH / 2);
            ctx.fillStyle = up ? '#39ff6a' : '#ff3b3b';
            ctx.fillText(`${up ? '▲' : '▼'}${Math.abs(seed * 9.9).toFixed(1)}%`, x + 82, barH / 2);
        }

        ctx.font = 'bold 12px "Helvetica Neue", Arial, sans-serif';
        ctx.textAlign = 'right';
        ctx.fillStyle = 'rgba(255,255,255,0.75)';
        ctx.fillText('PARODY · NOT FINANCIAL ADVICE · DO YOUR HOMEWORK', w - 12, barH + 18);
    }
}

function wrap(text: string, width: number): string[] {
    const lines: string[] = [];
    let line = '';
    for (const word of text.split(' ')) {
        if (line && (line + ' ' + word).length > width) {
            lines.push(line);
            line = word;
        } else {
            line = line ? `${line} ${word}` : word;
        }
    }
    lines.push(line);
    return lines;
}
