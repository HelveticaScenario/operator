import { coordinateModule, TRANSFORM_HELPER } from './transform';
import { KALEID_HELPER, MOD_HELPER, PIXELATE_HELPER } from './warp';

/*
 * The modulators, which read the whole input at coordinates that another
 * signal moves. `m` is the modulator, read whole: its red, green and
 * blue channels do the moving.
 */

/** A modulator module: `numbers` are its further inputs, and `m` the modulator. */
const modulator = (
    numbers: readonly string[],
    coordinates: (args: Record<string, string>) => string,
    helpers: readonly string[] = [],
) => coordinateModule(numbers, coordinates, helpers, ['m']);

/** Moves across by `amount` times the red channel and up by `amount` times the green. */
export const modulate = modulator(
    ['amount'],
    ({ m, amount }) => `uv + ${m}.rg * ${amount}`,
);

/** Scales about the center by `offset + multiple * channel`, red across and green up; a negative factor mirrors. */
export const modulateScale = modulator(
    ['multiple', 'offset'],
    ({ m, multiple, offset }) =>
        `video_mod_scale(uv, ${m}.rg, ${multiple}, ${offset})`,
    [
        `fn video_mod_scale(uv: vec2f, c: vec2f, multiple: f32, offset: f32) -> vec2f {
    let raw = vec2f(offset) + c * multiple;
    let factor = select(vec2f(-1.0), vec2f(1.0), raw >= vec2f(0.0)) * max(abs(raw), vec2f(0.00001));
    return (uv - vec2f(0.5)) / factor + vec2f(0.5);
}`,
    ],
);

/** Turns about the center by `offset + multiple * red`, in turns. */
export const modulateRotate = modulator(
    ['multiple', 'offset'],
    ({ m, multiple, offset }) =>
        `video_transform(uv, 1.0, ${offset} + ${m}.r * ${multiple}, vec2f(0.0))`,
    [TRANSFORM_HELPER],
);

/** Holds the input constant over a grid of `offset + multiple * channel` cells, red across and green up. */
export const modulatePixelate = modulator(
    ['multiple', 'offset'],
    ({ m, multiple, offset }) =>
        `video_pixelate(uv, vec2f(${offset}) + ${m}.rg * ${multiple})`,
    [PIXELATE_HELPER],
);

/** Mirrors into `sides` wedges, with the distance from the center pushed out by the red channel. */
export const modulateKaleid = modulator(
    ['sides'],
    ({ m, sides }) => `video_kaleid(uv, ${sides}, ${m}.r)`,
    [MOD_HELPER, KALEID_HELPER],
);

/** Pushes by the differences between the channels, `amount` pixels for a difference of full scale. */
export const modulateHue = modulator(
    ['amount'],
    ({ m, amount }) => `video_mod_hue(uv, ${m}, ${amount})`,
    [
        `fn video_mod_hue(uv: vec2f, c: vec4f, amount: f32) -> vec2f {
    return uv + vec2f(c.g - c.r, c.b - c.g) * amount / u.resolution;
}`,
    ],
);

/**
 * Tiles `x` by `y` times and slides every tile's picture by `offset` times
 * the red and green channels. The `step` terms follow Hydra's formula; they
 * add whole tiles, which `fract` removes, so every row and column slides alike.
 */
export const modulateRepeat = modulator(
    ['repeatX', 'repeatY', 'offsetX', 'offsetY'],
    ({ m, repeatX, repeatY, offsetX, offsetY }) =>
        `video_mod_repeat(uv, vec2f(${repeatX}, ${repeatY}), ${m}.rg, vec2f(${offsetX}, ${offsetY}))`,
    [
        MOD_HELPER,
        `fn video_mod_repeat(uv: vec2f, reps: vec2f, c: vec2f, offset: vec2f) -> vec2f {
    var st = uv * reps;
    st.x += step(1.0, video_mod(st.y, 2.0)) + c.x * offset.x;
    st.y += step(1.0, video_mod(st.x, 2.0)) + c.y * offset.y;
    return fract(st);
}`,
    ],
);

/** Tiles across, sliding every tile's picture up by `offset` times the red channel. */
export const modulateRepeatX = modulator(
    ['reps', 'offset'],
    ({ m, reps, offset }) =>
        `video_mod_repeat_x(uv, ${reps}, ${m}.r, ${offset})`,
    [
        MOD_HELPER,
        `fn video_mod_repeat_x(uv: vec2f, reps: f32, c: f32, offset: f32) -> vec2f {
    var st = uv * vec2f(reps, 1.0);
    st.y += step(1.0, video_mod(st.x, 2.0)) + c * offset;
    return fract(st);
}`,
    ],
);

/** Tiles up, sliding every tile's picture across by `offset` times the red channel. */
export const modulateRepeatY = modulator(
    ['reps', 'offset'],
    ({ m, reps, offset }) =>
        `video_mod_repeat_y(uv, ${reps}, ${m}.r, ${offset})`,
    [
        MOD_HELPER,
        `fn video_mod_repeat_y(uv: vec2f, reps: f32, c: f32, offset: f32) -> vec2f {
    var st = uv * vec2f(1.0, reps);
    st.x += step(1.0, video_mod(st.y, 2.0)) + c * offset;
    return fract(st);
}`,
    ],
);

/** Scrolls across by `scroll` times the red channel, plus `speed` of the frame per second, wrapping. */
export const modulateScrollX = modulator(
    ['scroll', 'speed'],
    ({ m, scroll, speed }) =>
        `fract(uv + vec2f(${m}.r * ${scroll} + u.time * ${speed}, 0.0))`,
);

/** Scrolls up by `scroll` times the red channel, plus `speed` of the frame per second, wrapping. */
export const modulateScrollY = modulator(
    ['scroll', 'speed'],
    ({ m, scroll, speed }) =>
        `fract(uv + vec2f(0.0, ${m}.r * ${scroll} + u.time * ${speed}))`,
);
