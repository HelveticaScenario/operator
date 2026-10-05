/**
 * Definitions of button controls created by the `$btn()` and `$toggleBtn()`
 * DSL functions.
 */

/** Voltage a pressed/latched button drives into its backing signal module;
 *  matches the engine's GATE_HIGH_VOLTAGE (crates/modular_core). */
export const GATE_HIGH_VOLTAGE = 5;

export type ButtonMode = 'gate' | 'toggle';

export interface ButtonDefinition {
    /** Backing signal module ID — the module the UI pokes with 0/5V */
    moduleId: string;
    /** Display label for the button */
    label: string;
    /** Labels of the enclosing `$cGroup`s, outermost first; [] at top level.
     *  Labels are unique within a group. */
    group: string[];
    mode: ButtonMode;
    /** Toggle state; always false for gate */
    value: boolean;
    /** Call site of the `$btn(...)` / `$toggleBtn(...)` call, as captured by
     *  V8: 1-based, with line-1 columns shifted by FIRST_LINE_COLUMN_OFFSET. */
    sourceLocation?: { line: number; column: number };
}
