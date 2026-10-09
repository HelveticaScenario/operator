/**
 * `video_transform`: maps frame coordinates `uv` through a zoom about the
 * center, a rotation in turns (positive is clockwise) and a shift, correcting
 * for the window's aspect ratio so rotation never shears. Used by ramps to move
 * their pattern and by feedback to resample the previous frame.
 */
export const TRANSFORM_HELPER = `fn video_transform(uv: vec2f, zoom: f32, rotate: f32, shift: vec2f) -> vec2f {
    let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
    let p = (uv - vec2f(0.5) - shift) * aspect / max(zoom, 0.00001);
    let a = rotate * 6.28318530718;
    let c = cos(a);
    let s = sin(a);
    return vec2f(c * p.x - s * p.y, s * p.x + c * p.y) / aspect + vec2f(0.5);
}`;
