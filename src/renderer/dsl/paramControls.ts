/**
 * Which control call to offer for a module param, from its schema metadata.
 *
 * The param's `signalType` picks the control: `gate` params get a
 * `$toggleBtn` or momentary `$btn`, `trig` params a `$btn`, and everything
 * else a `$slider`. A slider for a `pitch` param travels in notes and one for
 * a `frequency` param in hz; wrapping an existing note or hz literal keeps
 * the literal's unit. Unit sliders span two octaves either side of their
 * value, within the param's schema range.
 */
import {
    formatHzLiteral,
    hzToVolts,
    parseSliderValue,
    voltsToHz,
    voltsToNoteName,
} from '../../shared/dsl/sliderUnits';

/** The schema metadata a control is built from. */
export interface ParamSpec {
    signalType: string;
    defaultValue: number;
    minValue: number;
    maxValue: number;
}

/** A literal param value a control can take over. */
export type ParamLiteral =
    | { kind: 'number'; text: string; value: number }
    | { kind: 'string'; text: string; value: string };

export interface ControlChoice {
    title: string;
    /** The control call, e.g. `$slider('freq', 'a3', 'a1', 'a5')`, with
     *  generated strings in the configured quote. */
    text: string;
    /** The choice to apply by default when several are offered. */
    preferred: boolean;
}

/** Octaves a unit slider spans either side of its value. */
const UNIT_SPAN_OCTAVES = 2;

/** Gate-detection high threshold: a literal at or above it reads as on. */
const GATE_HIGH_THRESHOLD = 1;

const fmt = (n: number): string => Number(n.toPrecision(6)).toString();

/** Controls that can replace `literal` as the value of param `name`. The
 *  literal is kept as written; generated strings use `quote`. */
export function wrapChoices(
    name: string,
    literal: ParamLiteral,
    spec: ParamSpec,
    label: (base: string) => string,
    quote: string,
): ControlChoice[] {
    const q = (text: string) => `${quote}${text}${quote}`;
    if (literal.kind === 'string') {
        let parsed;
        try {
            parsed = parseSliderValue(literal.value);
        } catch {
            return [];
        }
        if (parsed.unit === 'number') {
            return [];
        }
        const unitName = parsed.unit === 'note' ? 'a note' : 'an hz';
        return [
            {
                preferred: true,
                text: unitSlider(
                    q(label(name)),
                    parsed.unit,
                    literal.text,
                    parsed.volts,
                    spec,
                    quote,
                ),
                title: `Wrap '${name}' in ${unitName} $slider`,
            },
        ];
    }
    switch (spec.signalType) {
        case 'gate':
            return [
                {
                    preferred: true,
                    text: toggleBtn(
                        q(label(name)),
                        literal.value >= GATE_HIGH_THRESHOLD,
                    ),
                    title: `Replace '${name}' with $toggleBtn`,
                },
                {
                    preferred: false,
                    text: btn(q(label(name))),
                    title: `Replace '${name}' with $btn`,
                },
            ];
        case 'trig':
            return [
                {
                    preferred: true,
                    text: btn(q(label(name))),
                    title: `Replace '${name}' with $btn`,
                },
            ];
        default:
            return [
                {
                    preferred: true,
                    text: numberSlider(
                        q(label(name)),
                        literal.text,
                        literal.value,
                        spec,
                    ),
                    title: `Wrap '${name}' in $slider`,
                },
            ];
    }
}

/** Controls for param `name` where the call does not set it yet, with
 *  generated strings in `quote`. */
export function addChoices(
    name: string,
    spec: ParamSpec,
    label: (base: string) => string,
    quote: string,
): ControlChoice[] {
    const q = (text: string) => `${quote}${text}${quote}`;
    const add = (what: string, text: string, preferred = false) => ({
        preferred,
        text,
        title: `Add ${what} for '${name}'`,
    });
    switch (spec.signalType) {
        case 'pitch': {
            const note = voltsToNoteName(spec.defaultValue);
            const volts = parseSliderValue(note).volts;
            return [
                add(
                    'note slider',
                    unitSlider(
                        q(label(name)),
                        'note',
                        q(note),
                        volts,
                        spec,
                        quote,
                    ),
                ),
            ];
        }
        case 'frequency': {
            // Two significant figures keep the generated hz literals short.
            const hz = Number(voltsToHz(spec.defaultValue).toPrecision(2));
            return [
                add(
                    'hz slider',
                    unitSlider(
                        q(label(name)),
                        'hz',
                        q(formatHzLiteral(hz)),
                        hzToVolts(hz),
                        spec,
                        quote,
                    ),
                ),
            ];
        }
        case 'gate':
            return [
                add('button', btn(q(label(name)))),
                add('toggle', toggleBtn(q(label(name)), false)),
            ];
        case 'trig':
            return [add('button', btn(q(label(name))))];
        default:
            return [
                add(
                    'slider',
                    numberSlider(
                        q(label(name)),
                        fmt(spec.defaultValue),
                        spec.defaultValue,
                        spec,
                    ),
                ),
            ];
    }
}

/** A volts slider over the schema range, widened to include the value.
 *  `label` and the value arrive as source text. */
function numberSlider(
    label: string,
    valueText: string,
    value: number,
    spec: ParamSpec,
): string {
    const min = fmt(Math.min(spec.minValue, value));
    const max = fmt(Math.max(spec.maxValue, value));
    return `$slider(${label}, ${valueText}, ${min}, ${max})`;
}

/**
 * A note or hz slider spanning UNIT_SPAN_OCTAVES either side of `volts`. A
 * side is clipped at a schema limit only when the value lies within it, so
 * a value past a limit (e.g. an LFO-rate '1hz') still gets its full span.
 */
function unitSlider(
    label: string,
    unit: 'note' | 'hz',
    valueText: string,
    volts: number,
    spec: ParamSpec,
    quote: string,
): string {
    const lo =
        volts >= spec.minValue
            ? Math.max(volts - UNIT_SPAN_OCTAVES, spec.minValue)
            : volts - UNIT_SPAN_OCTAVES;
    const hi =
        volts <= spec.maxValue
            ? Math.min(volts + UNIT_SPAN_OCTAVES, spec.maxValue)
            : volts + UNIT_SPAN_OCTAVES;
    const bound = (v: number) =>
        `${quote}${unit === 'note' ? voltsToNoteName(v) : formatHzLiteral(voltsToHz(v))}${quote}`;
    return `$slider(${label}, ${valueText}, ${bound(lo)}, ${bound(hi)})`;
}

function btn(label: string): string {
    return `$btn(${label})`;
}

function toggleBtn(label: string, initial: boolean): string {
    return `$toggleBtn(${label}, ${String(initial)})`;
}
