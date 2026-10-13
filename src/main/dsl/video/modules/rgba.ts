/** `video_luma`: the perceived brightness of a color's red, green and blue. */
export const LUMA_HELPER = `fn video_luma(c: vec4f) -> f32 {
    return dot(c.rgb, vec3f(0.299, 0.587, 0.114));
}`;

/** An `f32` expression as an opaque gray of that level. */
export const gray = (level: string): string => `vec4f(vec3f(${level}), 1.0)`;
