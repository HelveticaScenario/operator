import type { SliderUnit } from './sliderUnits';

/**
 * Definition of a slider control created by the `$slider()` DSL function.
 * For 'hz' and 'note' units, value/min/max are stored in V/Oct volts; the
 * renderer converts to the display unit.
 */
export interface SliderDefinition {
    /** Backing signal module ID */
    moduleId: string;
    /** Display label for the slider */
    label: string;
    /** Labels of the enclosing `$cGroup`s, outermost first; [] at top level.
     *  Labels are unique within a group. */
    group: string[];
    /** Current value */
    value: number;
    /** Minimum value */
    min: number;
    /** Maximum value */
    max: number;
    /** Unit the slider was written in */
    unit: SliderUnit;
    /** Call site of the `$slider(...)` call, as captured by V8: 1-based, with
     *  line-1 columns shifted by FIRST_LINE_COLUMN_OFFSET. */
    sourceLocation?: { line: number; column: number };
}
