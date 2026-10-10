import { TRANSFORM_HELPER } from './transform';
import { coordinateModule } from './warp';

/*
 * The modulators of Hydra, which read the whole input at coordinates that
 * another signal moves. `mr`, `mg` and `mb` are the red, green and blue of the
 * modulator, or its one value for a field; a modulator's channels are
 * fractions of full scale, so they run 0 to 1 here. Parameters that count
 * things or scale them are natural; the rest are fractions of 5 volts.
 */

/** Scales about the center by `offset + multiple * channel`, a factor per axis. */
export const modulateScale = coordinateModule(
    ['mr', 'mg', 'multiple', 'offset'],
    ['multiple', 'offset'],
    ({ mr, mg, multiple, offset }) =>
        `video_mod_scale(uv, vec2f(${mr}, ${mg}), ${multiple}, ${offset})`,
    [
        `fn video_mod_scale(uv: vec2f, c: vec2f, multiple: f32, offset: f32) -> vec2f {
    let factor = max(vec2f(offset) + c * multiple, vec2f(0.00001));
    return (uv - vec2f(0.5)) / factor + vec2f(0.5);
}`,
    ],
);

/** Turns about the center by `offset + multiple * red`, in turns. */
export const modulateRotate = coordinateModule(
    ['mr', 'multiple', 'offset'],
    ['multiple', 'offset'],
    ({ mr, multiple, offset }) =>
        `video_transform(uv, 1.0, ${offset} + ${mr} * ${multiple}, vec2f(0.0))`,
    [TRANSFORM_HELPER],
);

/** Holds the input constant over a grid of `offset + multiple * channel` cells per axis. */
export const modulatePixelate = coordinateModule(
    ['mr', 'mg', 'multiple', 'offset'],
    ['multiple', 'offset'],
    ({ mr, mg, multiple, offset }) =>
        `video_mod_pixelate(uv, vec2f(${offset} + ${mr} * ${multiple}, ${offset} + ${mg} * ${multiple}))`,
    [
        `fn video_mod_pixelate(uv: vec2f, cells: vec2f) -> vec2f {
    let grid = max(cells, vec2f(1.0));
    return floor(uv * grid + vec2f(0.5)) / grid;
}`,
    ],
);

/** Mirrors into `sides` wedges, with the distance from the center pushed out by the red channel. */
export const modulateKaleid = coordinateModule(
    ['mr', 'sides'],
    ['sides'],
    ({ mr, sides }) => `video_mod_kaleid(uv, ${sides}, ${mr})`,
    [
        `fn video_mod_kaleid(uv: vec2f, sides: f32, push: f32) -> vec2f {
    let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
    let p = (uv - vec2f(0.5)) * aspect;
    let wedge = 6.28318530718 / max(sides, 1.0);
    let turn = ((atan2(p.y, p.x) % wedge) + wedge) % wedge;
    let folded = abs(turn - wedge * 0.5);
    return vec2f(cos(folded), sin(folded)) * (length(p) + push) / aspect + vec2f(0.5);
}`,
    ],
);

/** Pushes by the differences between the channels, `amount` pixels for a difference of full scale. */
export const modulateHue = coordinateModule(
    ['mr', 'mg', 'mb', 'amount'],
    ['amount'],
    ({ mr, mg, mb, amount }) =>
        `uv + vec2f(${mg} - ${mr}, ${mb} - ${mg}) * ${amount} / u.resolution`,
);

/** Tiles `x` by `y` times, with alternate rows and columns shifted by `offset` times the channel. */
export const modulateRepeat = coordinateModule(
    ['mr', 'mg', 'repeatX', 'repeatY', 'offsetX', 'offsetY'],
    ['repeatX', 'repeatY'],
    ({ mr, mg, repeatX, repeatY, offsetX, offsetY }) =>
        `video_mod_repeat(uv, vec2f(${repeatX}, ${repeatY}), vec2f(${mr}, ${mg}), vec2f(${offsetX}, ${offsetY}))`,
    [
        `fn video_mod_repeat(uv: vec2f, reps: vec2f, c: vec2f, offset: vec2f) -> vec2f {
    var st = uv * reps;
    st.x += step(1.0, st.y % 2.0) + c.x * offset.x;
    st.y += step(1.0, st.x % 2.0) + c.y * offset.y;
    return fract(st);
}`,
    ],
);

/** Tiles across, shifting alternate columns up by `offset` times the red channel. */
export const modulateRepeatX = coordinateModule(
    ['mr', 'reps', 'offset'],
    ['reps'],
    ({ mr, reps, offset }) =>
        `video_mod_repeat_x(uv, ${reps}, ${mr}, ${offset})`,
    [
        `fn video_mod_repeat_x(uv: vec2f, reps: f32, c: f32, offset: f32) -> vec2f {
    var st = uv * vec2f(reps, 1.0);
    st.y += step(1.0, st.x % 2.0) + c * offset;
    return fract(st);
}`,
    ],
);

/** Tiles up, shifting alternate rows right by `offset` times the red channel. */
export const modulateRepeatY = coordinateModule(
    ['mr', 'reps', 'offset'],
    ['reps'],
    ({ mr, reps, offset }) =>
        `video_mod_repeat_y(uv, ${reps}, ${mr}, ${offset})`,
    [
        `fn video_mod_repeat_y(uv: vec2f, reps: f32, c: f32, offset: f32) -> vec2f {
    var st = uv * vec2f(1.0, reps);
    st.x += step(1.0, st.y % 2.0) + c * offset;
    return fract(st);
}`,
    ],
);

/** Scrolls across by `scroll` times the red channel, plus `speed` of the frame per second, wrapping. */
export const modulateScrollX = coordinateModule(
    ['mr', 'scroll', 'speed'],
    [],
    ({ mr, scroll, speed }) =>
        `fract(uv + vec2f(${mr} * ${scroll} + u.time * ${speed}, 0.0))`,
);

/** Scrolls up by `scroll` times the red channel, plus `speed` of the frame per second, wrapping. */
export const modulateScrollY = coordinateModule(
    ['mr', 'scroll', 'speed'],
    [],
    ({ mr, scroll, speed }) =>
        `fract(uv + vec2f(0.0, ${mr} * ${scroll} + u.time * ${speed}))`,
);
