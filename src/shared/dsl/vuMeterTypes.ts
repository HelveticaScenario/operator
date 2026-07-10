/**
 * One out-group's VU meter entry, emitted by the GraphBuilder on
 * `PatchGraph.vuMeters`. Rust reads the metering fields (moduleId / portName /
 * channels / muteModuleId); the rest is renderer metadata that rides through
 * `appliedPatch` untouched (same contract as `Scope.sourceLocation`).
 */
export interface VuMeterDef {
    /**
     * Stable identity: the label if present, else the next positional
     * `out N` not claimed by a label. Unique across every meter, including
     * the master's reserved `__main__`.
     */
    key: string;
    label: string | null;
    /** Module whose output port is metered (the post-gain, pre-mute tap). */
    moduleId: string;
    portName: string;
    /** 1 (mono) or 2 (stereo). */
    channels: number;
    /**
     * `$signal` driving the mute gate: source 5 = audible, 0 = silenced.
     * Absent for the end-of-chain master meter, which has no mute/solo
     * (undefined rather than null: napi's Option only accepts undefined).
     */
    muteModuleId?: string;
    baseChannel: number;
    mute: boolean;
    solo: boolean;
    /**
     * Pan position (-5 left … +5 right) when the panel's knob controls it —
     * stereo outs whose `pan` option is a number or absent. Null when pan is
     * signal-driven or the out is mono (no knob).
     */
    pan: number | null;
    /** `$signal` driving the stereo mixer's pan; null when `pan` is null. */
    panModuleId: string | null;
    /**
     * True when pan is signal-driven: the knob renders locked (externally
     * controlled) and tracks the live value from the meter frames.
     */
    panLocked: boolean;
    /**
     * Output gain in DSL units (audio taper; 5 = unity) when the panel's
     * fader controls it — outs whose `gain` option is a number or absent.
     * Null when gain is signal-driven.
     */
    gain: number | null;
    /** `$signal` driving the gain stage; null when `gain` is null. */
    gainModuleId: string | null;
    /** True when gain is signal-driven: the fader is locked and live. */
    gainLocked: boolean;
    /**
     * Live-display taps for locked controls (Rust samples them into the
     * meter frames). Absent when the control is editable or untappable.
     */
    panSource?: { moduleId: string; portName: string; channel: number };
    gainSource?: { moduleId: string; portName: string; channel: number };
    /** True for the end-of-chain master meter pinned to the panel's right. */
    main?: boolean;
    /**
     * Location of the `.out(...)` / `.outMono(...)` call in the DSL source.
     * Absent when several out groups share the call site (a loop or helper):
     * a source edit there would rewrite every instance, so such meters'
     * controls act on the live graph only.
     */
    sourceLocation?: { line: number; column: number };
}

/**
 * Code-side control values that diverge from the running audio after a
 * Ctrl/Cmd (code-only) panel gesture. Renderer-only: a missing property
 * means code and audio agree for that control. Cleared per control when a
 * plain gesture writes both sides, and wholesale when a patch update
 * applies (the compiled source re-syncs audio to code).
 */
export interface VuMeterGhost {
    mute?: boolean;
    solo?: boolean;
    /** Pan position in the source, -5…+5. */
    pan?: number;
    /** Out gain in the source (DSL units, 0…10). */
    gain?: number;
}

/** Exponent of the perceptual (audio-taper) gain curve used by out gains. */
export const GAIN_CURVE_EXP = 3;

/** Out gain value producing unity amplitude: $curve(5, 3) = 5 → scale 5/5. */
export const UNITY_OUT_GAIN = 5;

/**
 * Out gain (DSL units, 0…10) → dB. The gain passes through
 * `$curve(g, GAIN_CURVE_EXP)` (5·(g/5)^exp) into a `$scaleAndShift` scale
 * (amplitude g′/5), so amplitude = (g/5)^exp. Returns -Infinity at 0.
 */
export function outGainToDb(gain: number): number {
    if (gain <= 0) {
        return -Infinity;
    }
    return 20 * GAIN_CURVE_EXP * Math.log10(gain / UNITY_OUT_GAIN);
}

/** dB → out gain (DSL units). -Infinity (or below the floor) maps to 0. */
export function dbToOutGain(db: number): number {
    if (!Number.isFinite(db)) {
        return 0;
    }
    return UNITY_OUT_GAIN * Math.pow(10, db / (20 * GAIN_CURVE_EXP));
}
