/** Module type whose `ui: true` param requests an inline EQ panel. */
export const EQ_MODULE_TYPE = '$unstable.eq';

/**
 * Frame window of the headless spectrum scope. Short enough that the scope's
 * 1024-sample ring records every sample (no decimation) at rates up to
 * ~102 kHz, which the renderer's FFT needs.
 */
export const EQ_SPECTRUM_MS_PER_FRAME = 10;

/**
 * One `ui: true` EQ, emitted by the GraphBuilder beside the patch (like
 * sliders). The panel reads the module's live params from `appliedPatch`.
 */
export interface EqPanelDef {
    /** Stable renderer key, `eq:<moduleId>`. */
    key: string;
    moduleId: string;
    /**
     * Location of the `eq` method name in the DSL source. Absent when several
     * EQs share the call site (a loop or helper): a source edit there would
     * rewrite every instance, so such panels act on the live graph only.
     */
    sourceLocation?: { line: number; column: number };
    /**
     * Location used only to place the panel's view zone. Present whenever the
     * call site is known, including shared call sites.
     */
    zoneLocation?: { line: number; column: number };
    /**
     * Headless scopes feeding the panel's spectrum: output channel 0 for a
     * shared section list, or channel i for per-channel list i.
     */
    spectrum: {
        moduleId: string;
        portName: string;
        channels: number[];
        msPerFrame: number;
    };
}
