// Sample-buffer dimensions and the GPU Lanczos upsampler kernel for the XY
// scope. SCOPE_XY_CAPACITY mirrors the Rust ring size (audio.rs); MAX_TRACES
// caps how many traces the pipeline draws (and the DSL pair count).

export const MAX_TRACES = 16;
export const SCOPE_XY_CAPACITY = 2048;

// Lanczos upsampler tuning: each input sample produces STEPS interpolated
// outputs using a sinc window of half-width RADIUS source samples. Output
// count = N*STEPS + 1.
export const UPSAMPLE_STEPS = 6;
export const UPSAMPLE_RADIUS = 8;
const UPSAMPLE_LANCZOS_TWEAK = 1.5;
export const UPSAMPLED_LEN = SCOPE_XY_CAPACITY * UPSAMPLE_STEPS + 1;

export function buildLanczosKernel(a: number, steps: number): Float32Array {
    const kernel = new Float32Array(a * steps);
    kernel[0] = 1;
    for (let i = 1; i < kernel.length; i++) {
        const piX = (Math.PI * i) / steps;
        const sinc = Math.sin(piX) / piX;
        const window = (a * Math.sin(piX / a)) / piX;
        kernel[i] = sinc * Math.pow(window, UPSAMPLE_LANCZOS_TWEAK);
    }
    return kernel;
}
