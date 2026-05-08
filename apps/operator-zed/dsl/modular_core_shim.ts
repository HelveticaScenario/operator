// Shim for `@modular/core`.
//
// The real package (crates/modular/index.js) is the N-API addon entry — it
// requires a Node runtime and the compiled `.node` binary. operator-zed runs
// the DSL inside a deno_core JsRuntime, where neither is available, so the
// esbuild bundle aliases `@modular/core` to this file.
//
// Pure-JS surface only:
//   - Type re-exports: handled by the .d.ts re-shape; unused at runtime.
//   - Runtime values: ScopeMode (string union → no value needed),
//     deriveChannelCount + getReservedOutputNames → call into Rust ops.

declare const Deno: {
    core: {
        ops: Record<string, (...args: unknown[]) => unknown>;
    };
};

export type ModuleSchema = unknown;
export type ModuleState = unknown;
export type PatchGraph = unknown;
export type Scope = unknown;
export type ScopeMode = 'Wait' | 'Roll';

export interface DeriveChannelCountResult {
    channelCount?: number | null;
    errors?: Array<{ message: string; params: string[] }> | null;
}

export function deriveChannelCount(
    moduleType: string,
    params: unknown,
): DeriveChannelCountResult {
    return Deno.core.ops.op_modz_derive_channel_count(
        moduleType,
        params,
    ) as DeriveChannelCountResult;
}

export function getReservedOutputNames(): string[] {
    return Deno.core.ops.op_modz_reserved_output_names() as string[];
}
