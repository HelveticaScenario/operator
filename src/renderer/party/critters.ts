/**
 * The critters that dance along the bottom of the editor. Each is drawn with
 * canvas paths, standing on its feet at the origin, about 100 units tall
 * (up is -y), and bobs, sways and flails with the music.
 */

export interface Groove {
    /** Seconds since the layer started. */
    t: number;
    /** Loudness, 0 to 1. */
    level: number;
    /** 1 on a beat, decaying between beats. */
    pulse: number;
    /** Each critter's own offset, so they do not dance in lockstep. */
    phase: number;
    /** Seconds left in a strong-beat special move, or 0. */
    special: number;
}

export interface Critter {
    name: string;
    /** Units wide the critter takes up in the line. */
    width: number;
    /** Where a cowboy hat sits: on the crown, and how big. */
    hat: { x: number; y: number; scale: number; tilt?: number };
    quips: string[];
    draw(ctx: CanvasRenderingContext2D, g: Groove): void;
}

const TAU = Math.PI * 2;

function ellipse(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    rx: number,
    ry: number,
    fill: string,
    rotation = 0,
    stroke = true,
): void {
    ctx.beginPath();
    ctx.ellipse(x, y, rx, ry, rotation, 0, TAU);
    ctx.fillStyle = fill;
    ctx.fill();
    if (stroke) ctx.stroke();
}

function limb(
    ctx: CanvasRenderingContext2D,
    x1: number,
    y1: number,
    x2: number,
    y2: number,
    width: number,
    color: string,
): void {
    ctx.save();
    ctx.lineCap = 'round';
    ctx.lineWidth = width + 3;
    ctx.strokeStyle = '#111';
    ctx.beginPath();
    ctx.moveTo(x1, y1);
    ctx.lineTo(x2, y2);
    ctx.stroke();
    ctx.lineWidth = width;
    ctx.strokeStyle = color;
    ctx.stroke();
    ctx.restore();
}

/** A two-segment arm from the shoulder, swinging by `angle` (radians). */
function arm(
    ctx: CanvasRenderingContext2D,
    sx: number,
    sy: number,
    angle: number,
    length: number,
    width: number,
    sleeve: string,
    hand: string,
    side: 1 | -1,
): void {
    const ex = sx + Math.sin(angle) * length * 0.55 * side;
    const ey = sy + Math.cos(angle) * length * 0.55;
    const hx = ex + Math.sin(angle * 1.6) * length * 0.45 * side;
    const hy = ey + Math.cos(angle * 1.6) * length * 0.45;
    limb(ctx, sx, sy, ex, ey, width, sleeve);
    limb(ctx, ex, ey, hx, hy, width * 0.8, sleeve);
    ellipse(ctx, hx, hy, width * 0.55, width * 0.55, hand);
}

function poly(ctx: CanvasRenderingContext2D, points: number[], fill: string, stroke = true): void {
    ctx.beginPath();
    ctx.moveTo(points[0], points[1]);
    for (let i = 2; i < points.length; i += 2) ctx.lineTo(points[i], points[i + 1]);
    ctx.closePath();
    ctx.fillStyle = fill;
    ctx.fill();
    if (stroke) ctx.stroke();
}

function eye(ctx: CanvasRenderingContext2D, x: number, y: number, r: number, look = 0): void {
    ellipse(ctx, x, y, r, r * 1.15, '#fff');
    ellipse(ctx, x + look * r * 0.35, y + r * 0.1, r * 0.42, r * 0.5, '#111', 0, false);
}

/** Flail amount for arms: always a little, a lot when it is loud. */
const flail = (g: Groove, speed = 7, extra = 0) =>
    Math.sin(g.t * speed + g.phase + extra) * (0.4 + g.level * 1.6);

const seagull: Critter = {
    name: 'seagull',
    width: 80,
    hat: { x: 16, y: -70, scale: 0.6, tilt: 0.2 },
    quips: ['SQUAWK!', 'chips??', 'CAW CAW', 'mine. MINE.'],
    draw(ctx, g) {
        const flap = Math.sin(g.t * (8 + g.level * 14) + g.phase) * (0.3 + g.level);
        limb(ctx, -6, -18, -8, 0, 3, '#f0a030');
        limb(ctx, 6, -18, 8, 0, 3, '#f0a030');
        poly(ctx, [-26, -38, -44, -46, -42, -30], '#9aa3ad');
        ellipse(ctx, 0, -34, 28, 17, '#fafafa');
        // Wing, flapping from the shoulder.
        ctx.save();
        ctx.translate(-4, -40);
        ctx.rotate(-0.3 - flap);
        poly(ctx, [0, 0, -38, -8, -30, 6, -10, 8], '#8f99a3');
        poly(ctx, [-30, -6, -40, -8, -34, 4], '#222', false);
        ctx.restore();
        ellipse(ctx, 18, -58, 13, 12, '#fafafa');
        eye(ctx, 22, -61, 3.2, 1);
        const open = g.pulse * 6;
        poly(ctx, [28, -60, 44, -57 - open * 0.2, 28, -55], '#f6c026');
        poly(ctx, [28, -55, 42, -54 + open, 28, -52 + open * 0.5], '#f6c026');
        ellipse(ctx, 38, -55 + open * 0.5, 2, 2, '#e23', 0, false);
    },
};

const koala: Critter = {
    name: 'koala',
    width: 80,
    hat: { x: 0, y: -94, scale: 0.85 },
    quips: ["G'day!", 'eucalyptus o’clock', 'nom nom', 'zzz... BEAT DROP'],
    draw(ctx, g) {
        const bob = Math.sin(g.t * 5 + g.phase) * 0.12 * (0.3 + g.level);
        limb(ctx, -12, -12, -14, 0, 9, '#8d939a');
        limb(ctx, 12, -12, 14, 0, 9, '#8d939a');
        ellipse(ctx, 0, -32, 25, 28, '#9aa1a8');
        ellipse(ctx, 0, -28, 15, 18, '#d9dde0', 0, false);
        // A gum branch, waved like a glow stick.
        ctx.save();
        ctx.translate(18, -42);
        ctx.rotate(-0.6 + flail(g, 6) * 0.5);
        limb(ctx, 0, 0, 0, -36, 3, '#7a5230');
        for (let i = 0; i < 4; i++) {
            ellipse(ctx, i % 2 ? 7 : -7, -12 - i * 7, 7, 3.5, '#5fae5a', i % 2 ? 0.5 : -0.5);
        }
        ctx.restore();
        arm(ctx, 18, -44, -2.4 + flail(g, 6) * 0.3, 18, 8, '#8d939a', '#7a8087', 1);
        arm(ctx, -18, -44, 0.6 + flail(g, 6, 2) * 0.4, 18, 8, '#8d939a', '#7a8087', -1);
        ctx.save();
        ctx.translate(0, -70);
        ctx.rotate(bob);
        ellipse(ctx, -22, -14, 14, 13, '#9aa1a8');
        ellipse(ctx, 22, -14, 14, 13, '#9aa1a8');
        ellipse(ctx, -22, -14, 8, 7, '#f1c6d0', 0, false);
        ellipse(ctx, 22, -14, 8, 7, '#f1c6d0', 0, false);
        ellipse(ctx, 0, 0, 23, 20, '#a3aab1');
        eye(ctx, -9, -4, 3);
        eye(ctx, 9, -4, 3);
        ellipse(ctx, 0, 4, 7, 9, '#2a2a2a');
        ellipse(ctx, -2, 1, 2, 2.5, '#777', 0, false);
        ctx.restore();
    },
};

const ed: Critter = {
    name: 'Ed',
    width: 70,
    hat: { x: 0, y: -122, scale: 0.85 },
    quips: ['BUTTERED TOAST!', 'gravy!!', 'I am a monster, Eddy!', 'Ed boy!'],
    draw(ctx, g) {
        const skin = '#e9d89a';
        limb(ctx, -8, -40, -11, -4, 11, '#3b4f9c');
        limb(ctx, 8, -40, 11, -4, 11, '#3b4f9c');
        ellipse(ctx, -13, -2, 9, 4, '#222');
        ellipse(ctx, 13, -2, 9, 4, '#222');
        // Striped shirt under a green jacket.
        poly(ctx, [-18, -86, 18, -86, 16, -38, -16, -38], '#f5f5f5');
        ctx.save();
        ctx.beginPath();
        ctx.rect(-8, -86, 16, 48);
        ctx.clip();
        for (let y = -86; y < -38; y += 7) {
            ctx.fillStyle = '#d62f2f';
            ctx.fillRect(-8, y, 16, 3.5);
        }
        ctx.restore();
        poly(ctx, [-20, -86, -8, -86, -8, -38, -18, -38], '#4c8a3c');
        poly(ctx, [20, -86, 8, -86, 8, -38, 18, -38], '#4c8a3c');
        arm(ctx, -19, -82, 2.6 + flail(g, 9) * 0.8, 40, 9, '#4c8a3c', skin, -1);
        arm(ctx, 19, -82, 2.6 + flail(g, 9, Math.PI) * 0.8, 40, 9, '#4c8a3c', skin, 1);
        ctx.save();
        ctx.translate(0, -100);
        ctx.rotate(Math.sin(g.t * 4 + g.phase) * 0.1);
        // Flat-topped head with a jutting chin.
        poly(ctx, [-13, -20, 13, -20, 14, 8, 8, 16, -8, 16, -14, 8], skin);
        ctx.fillStyle = '#b89a3a';
        ctx.fillRect(-13, -21, 26, 4);
        limb(ctx, -10, -9, 10, -9, 2.5, '#222');
        eye(ctx, -6, -4, 3.2, Math.sin(g.t * 3));
        eye(ctx, 6, -4, 3.2, -Math.sin(g.t * 3));
        ctx.beginPath();
        ctx.arc(0, 6, 8 + g.pulse * 2, 0, Math.PI);
        ctx.fillStyle = '#5a1d1d';
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = '#fff';
        ctx.fillRect(-6, 6, 12, 2.5);
        ellipse(ctx, 3, 12 + g.pulse * 3, 3, 3 + g.pulse * 2, '#e66', 0, false);
        ctx.restore();
    },
};

const edd: Critter = {
    name: 'Edd',
    width: 60,
    hat: { x: 0, y: -104, scale: 0.8 },
    quips: ['Oh dear.', 'Good lord!', 'Fascinating!', 'This is unsanitary!'],
    draw(ctx, g) {
        const skin = '#f2cfa8';
        limb(ctx, -6, -30, -7, -3, 5, skin);
        limb(ctx, 6, -30, 7, -3, 5, skin);
        ellipse(ctx, -6, -8, 3.5, 4, '#e33', 0, false);
        ellipse(ctx, 6, -8, 3.5, 4, '#e33', 0, false);
        ellipse(ctx, -8, -2, 7, 3.5, '#222');
        ellipse(ctx, 8, -2, 7, 3.5, '#222');
        poly(ctx, [-11, -40, 11, -40, 12, -28, -12, -28], '#7a3e9a');
        poly(ctx, [-11, -70, 11, -70, 11, -40, -11, -40], '#d92d2d');
        // Neat little pointing dance.
        arm(ctx, -11, -66, 0.4 + flail(g, 5) * 0.6, 30, 5, '#d92d2d', skin, -1);
        arm(ctx, 11, -66, 3.0 + Math.sin(g.t * 5 + g.phase) * 0.3, 30, 5, '#d92d2d', skin, 1);
        ctx.save();
        ctx.translate(0, -84);
        ctx.rotate(Math.sin(g.t * 5 + g.phase) * 0.08);
        ellipse(ctx, 0, 2, 12, 13, skin);
        // The black sock hat with its white stripes.
        poly(ctx, [-13, -2, -12, -12, -6, -17, 6, -17, 12, -12, 13, -2, 13, 6, -13, 6], '#111');
        ctx.fillStyle = '#eee';
        ctx.fillRect(-13, -1, 4, 6);
        ctx.fillRect(9, -1, 4, 6);
        eye(ctx, -5, 8, 2.8, 0.5);
        eye(ctx, 5, 8, 2.8, 0.5);
        ctx.beginPath();
        ctx.arc(0, 13, 4, 0, Math.PI);
        ctx.fillStyle = '#5a1d1d';
        ctx.fill();
        ctx.fillStyle = '#fff';
        ctx.fillRect(-2.5, 13, 2, 2);
        ctx.fillRect(0.5, 13, 2, 2);
        ctx.restore();
    },
};

const eddy: Critter = {
    name: 'Eddy',
    width: 60,
    hat: { x: 0, y: -96, scale: 0.85 },
    quips: ['SCAM TIME!', 'Jawbreakers!!', 'Sock it to me!', 'Shut up, Ed!'],
    draw(ctx, g) {
        const skin = '#f0c9a0';
        limb(ctx, -6, -24, -8, -3, 8, '#3d63b5');
        limb(ctx, 6, -24, 8, -3, 8, '#3d63b5');
        ellipse(ctx, -9, -2, 7, 3.5, '#a33');
        ellipse(ctx, 9, -2, 7, 3.5, '#a33');
        poly(ctx, [-14, -54, 14, -54, 13, -24, -13, -24], '#f2d43a');
        ctx.fillStyle = '#7a2fa0';
        ctx.fillRect(-14, -44, 28, 4);
        // Fists pumping on the beat.
        const pump = -g.pulse * 1.2;
        arm(ctx, -14, -51, 3.0 + pump + flail(g, 8) * 0.3, 24, 6, '#f2d43a', skin, -1);
        arm(ctx, 14, -51, 3.0 + pump + flail(g, 8, 1) * 0.3, 24, 6, '#f2d43a', skin, 1);
        ctx.save();
        ctx.translate(0, -72);
        ctx.rotate(Math.sin(g.t * 6 + g.phase) * 0.12);
        ellipse(ctx, 0, 0, 17, 16, skin);
        ctx.lineWidth = 1.5;
        for (const x of [-4, 0, 4]) {
            ctx.beginPath();
            ctx.moveTo(x, -15);
            ctx.quadraticCurveTo(x + 2, -24, x + 4, -20);
            ctx.stroke();
        }
        ctx.lineWidth = 2;
        eye(ctx, -6, -4, 3);
        eye(ctx, 6, -4, 3);
        // The big scheming grin.
        ctx.beginPath();
        ctx.moveTo(-11, 4);
        ctx.quadraticCurveTo(0, 18 + g.pulse * 4, 11, 4);
        ctx.closePath();
        ctx.fillStyle = '#5a1d1d';
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = '#fff';
        ctx.fillRect(-8, 4, 16, 3);
        ctx.restore();
    },
};

const sonic: Critter = {
    name: 'Sonic',
    width: 70,
    hat: { x: 2, y: -92, scale: 0.85, tilt: -0.15 },
    quips: ['Gotta go fast!', 'Too slow!', 'Way past cool!', '*taps foot*'],
    draw(ctx, g) {
        const blue = '#1f5fe0';
        const peach = '#f3c99a';
        if (g.special > 0) {
            // Spin dash: a blue ball with a blur trail.
            const spin = g.t * 40;
            ctx.save();
            ctx.translate(0, -26);
            for (let i = 1; i <= 3; i++) {
                ctx.globalAlpha = 0.18 * (4 - i);
                ellipse(ctx, -i * 14, 0, 24, 24, blue, 0, false);
            }
            ctx.globalAlpha = 1;
            ctx.rotate(spin);
            ellipse(ctx, 0, 0, 24, 24, blue);
            ctx.strokeStyle = '#7aa7ff';
            for (let i = 0; i < 4; i++) {
                ctx.beginPath();
                ctx.arc(0, 0, 16, (i * TAU) / 4, (i * TAU) / 4 + 0.9);
                ctx.stroke();
            }
            ctx.restore();
            return;
        }
        const run = Math.sin(g.t * (6 + g.level * 10) + g.phase) * (0.3 + g.level * 0.9);
        limb(ctx, -6, -26, -8 - run * 8, -6, 5, blue);
        limb(ctx, 6, -26, 8 + run * 8, -6, 5, blue);
        for (const [x, d] of [
            [-10 - run * 8, -1],
            [10 + run * 8, 1],
        ]) {
            ellipse(ctx, x + d * 4, -4, 11, 6, '#e0262b');
            ctx.fillStyle = '#fff';
            ctx.fillRect(x + d * 4 - 2, -9, 4, 9);
        }
        ellipse(ctx, 0, -38, 14, 16, blue);
        ellipse(ctx, 2, -36, 8, 11, peach, 0, false);
        arm(ctx, -12, -46, 0.5 + flail(g, 8) * 0.7, 24, 4, peach, '#fff', -1);
        arm(ctx, 12, -46, 2.8 + flail(g, 8, 2) * 0.5, 24, 4, peach, '#fff', 1);
        ctx.save();
        ctx.translate(0, -70);
        ctx.rotate(Math.sin(g.t * 5 + g.phase) * 0.1);
        // Swept-back quills.
        poly(ctx, [-6, -18, -40, -22, -12, -8, -44, -2, -12, 4, -36, 18, -4, 12], blue);
        ellipse(ctx, 0, 0, 20, 19, blue);
        poly(ctx, [-12, -14, -8, -34, 2, -16], blue);
        poly(ctx, [8, -14, 16, -30, 16, -10], blue);
        ellipse(ctx, 10, 9, 13, 8, peach);
        // The joined eyes, white on blue.
        ellipse(ctx, 6, -3, 11, 9, '#fff', 0, true);
        ellipse(ctx, 4, -2, 3, 5, '#2e9f3a', 0, false);
        ellipse(ctx, 11, -2, 3, 5, '#2e9f3a', 0, false);
        ellipse(ctx, 4.5, -2, 1.5, 3, '#111', 0, false);
        ellipse(ctx, 11.5, -2, 1.5, 3, '#111', 0, false);
        ellipse(ctx, 22, 6, 3, 2.5, '#111', 0, false);
        ctx.beginPath();
        ctx.moveTo(6, 12);
        ctx.quadraticCurveTo(13, 17, 20, 11);
        ctx.stroke();
        ctx.restore();
    },
};

const barack: Critter = {
    name: 'Barack',
    width: 70,
    hat: { x: 0, y: -122, scale: 0.9 },
    quips: ['Yes we can!', 'Let me be clear:', 'this beat? fired up.', 'Hope.'],
    draw(ctx, g) {
        const skin = '#9b6b45';
        const suit = '#20242e';
        limb(ctx, -8, -44, -10, -4, 11, suit);
        limb(ctx, 8, -44, 10, -4, 11, suit);
        ellipse(ctx, -12, -2, 9, 4, '#111');
        ellipse(ctx, 12, -2, 9, 4, '#111');
        poly(ctx, [-20, -92, 20, -92, 17, -42, -17, -42], suit);
        poly(ctx, [-7, -92, 7, -92, 0, -66], '#fff');
        poly(ctx, [-2.5, -90, 2.5, -90, 3.5, -64, 0, -58, -3.5, -64], '#2f5fb3');
        // One arm waves to the crowd, the other keeps the groove.
        arm(ctx, 20, -88, 3.4 + Math.sin(g.t * 6 + g.phase) * 0.35, 40, 9, suit, skin, 1);
        arm(ctx, -20, -88, 0.3 + flail(g, 5) * 0.5, 40, 9, suit, skin, -1);
        ctx.save();
        ctx.translate(0, -106);
        ctx.rotate(Math.sin(g.t * 4 + g.phase) * 0.07);
        ellipse(ctx, -15, 2, 5, 7, skin);
        ellipse(ctx, 15, 2, 5, 7, skin);
        ellipse(ctx, 0, 0, 14, 17, skin);
        ctx.beginPath();
        ctx.ellipse(0, -6, 14.5, 12, 0, Math.PI, TAU);
        ctx.fillStyle = '#4a4a4a';
        ctx.fill();
        ctx.stroke();
        ellipse(ctx, -5, -1, 2.5, 2.5, '#111', 0, false);
        ellipse(ctx, 5, -1, 2.5, 2.5, '#111', 0, false);
        limb(ctx, -8, -5, -2, -5.5, 1.5, '#222');
        limb(ctx, 2, -5.5, 8, -5, 1.5, '#222');
        // The big grin.
        ctx.beginPath();
        ctx.moveTo(-8, 7);
        ctx.quadraticCurveTo(0, 15 + g.pulse * 2, 8, 7);
        ctx.closePath();
        ctx.fillStyle = '#fff';
        ctx.fill();
        ctx.stroke();
        ctx.restore();
    },
};

export const CRITTERS: Critter[] = [seagull, koala, ed, edd, eddy, sonic, barack];

/** A cowboy hat, brim centred on the origin. */
export function drawCowboyHat(ctx: CanvasRenderingContext2D, scale: number, tilt = 0): void {
    ctx.save();
    ctx.scale(scale, scale);
    ctx.rotate(tilt);
    ctx.lineWidth = 2;
    ctx.strokeStyle = '#2b1606';
    ctx.beginPath();
    ctx.moveTo(-30, 0);
    ctx.quadraticCurveTo(-24, 6, 0, 5);
    ctx.quadraticCurveTo(24, 6, 30, 0);
    ctx.quadraticCurveTo(34, -8, 26, -6);
    ctx.quadraticCurveTo(0, 0, -26, -6);
    ctx.quadraticCurveTo(-34, -8, -30, 0);
    ctx.fillStyle = '#8b5a2b';
    ctx.fill();
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(-15, -3);
    ctx.quadraticCurveTo(-17, -24, -8, -24);
    ctx.quadraticCurveTo(0, -20, 8, -24);
    ctx.quadraticCurveTo(17, -24, 15, -3);
    ctx.closePath();
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = '#3a2210';
    ctx.fillRect(-15, -8, 30, 4);
    ctx.restore();
}

/** A seagull gliding overhead, wings in a shallow W. */
export function drawFlyingGull(ctx: CanvasRenderingContext2D, flap: number): void {
    ctx.lineWidth = 2;
    ctx.strokeStyle = '#111';
    ctx.beginPath();
    ctx.moveTo(-26, -6 - flap * 10);
    ctx.quadraticCurveTo(-12, -14 - flap * 6, 0, 0);
    ctx.quadraticCurveTo(12, -14 - flap * 6, 26, -6 - flap * 10);
    ctx.quadraticCurveTo(12, -6, 0, 4);
    ctx.quadraticCurveTo(-12, -6, -26, -6 - flap * 10);
    ctx.fillStyle = '#f4f4f4';
    ctx.fill();
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(-26, -6 - flap * 10);
    ctx.lineTo(-20, -8 - flap * 9);
    ctx.moveTo(26, -6 - flap * 10);
    ctx.lineTo(20, -8 - flap * 9);
    ctx.stroke();
}
