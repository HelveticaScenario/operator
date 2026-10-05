import { useCallback, useState } from 'react';
import type { SliderDefinition } from '../../shared/dsl/sliderTypes';
import type { ButtonDefinition } from '../../shared/dsl/buttonTypes';
import {
    snapVoltsToSemitone,
    voltsToHz,
    voltsToNoteName,
} from '../../shared/dsl/sliderUnits';
import './ControlPanel.css';

interface ControlPanelProps {
    sliders: SliderDefinition[];
    buttons: ButtonDefinition[];
    onSliderChange: (label: string, newValue: number) => void;
    onButtonChange: (label: string, pressed: boolean) => void;
}

export function ControlPanel({
    sliders,
    buttons,
    onSliderChange,
    onButtonChange,
}: ControlPanelProps) {
    if (sliders.length === 0 && buttons.length === 0) {
        return (
            <div className="control-panel control-panel-empty">
                <div className="control-panel-placeholder">
                    <p>No controls defined.</p>
                    <p className="control-panel-hint">
                        Use <code>$slider(label, value, min, max)</code>,{' '}
                        <code>$btn(label)</code>, or{' '}
                        <code>$toggleBtn(label, initial)</code> in your patch.
                    </p>
                </div>
            </div>
        );
    }

    return (
        <div className="control-panel">
            {buttons.length > 0 && (
                <div className="control-panel-buttons">
                    {buttons.map((b) => (
                        <ButtonControl
                            key={b.label}
                            button={b}
                            onChange={onButtonChange}
                        />
                    ))}
                </div>
            )}
            <div className="control-panel-sliders">
                {sliders.map((s) => (
                    <SliderControl
                        key={s.label}
                        slider={s}
                        onChange={onSliderChange}
                    />
                ))}
            </div>
        </div>
    );
}

interface SliderControlProps {
    slider: SliderDefinition;
    onChange: (label: string, newValue: number) => void;
}

function SliderControl({ slider, onChange }: SliderControlProps) {
    const [localValue, setLocalValue] = useState(slider.value);

    // Sync local state when slider definition changes (e.g., re-execution)
    const [prevValue, setPrevValue] = useState(slider.value);
    if (slider.value !== prevValue) {
        setLocalValue(slider.value);
        setPrevValue(slider.value);
    }

    // Note sliders step in semitones; value/min/max are V/Oct volts.
    const step =
        slider.unit === 'note' ? 1 / 12 : (slider.max - slider.min) / 1000;

    const handleInput = useCallback(
        (e: React.ChangeEvent<HTMLInputElement>) => {
            const raw = parseFloat(e.currentTarget.value);
            const newValue =
                slider.unit === 'note' ? snapVoltsToSemitone(raw) : raw;
            setLocalValue(newValue);
            onChange(slider.label, newValue);
        },
        [slider.label, slider.unit, onChange],
    );

    const formatValue = (v: number): string => {
        if (slider.unit === 'hz') {
            return `${Number(voltsToHz(v).toPrecision(4))} Hz`;
        }
        if (slider.unit === 'note') {
            return voltsToNoteName(v);
        }
        return Number(v.toPrecision(4)).toString();
    };

    return (
        <div className="slider-control">
            <div className="slider-header">
                <span className="slider-label">{slider.label}</span>
                <span className="slider-value">{formatValue(localValue)}</span>
            </div>
            <input
                type="range"
                className="slider-input"
                min={slider.min}
                max={slider.max}
                step={step}
                value={localValue}
                onChange={handleInput}
            />
            <div className="slider-range">
                <span>{formatValue(slider.min)}</span>
                <span>{formatValue(slider.max)}</span>
            </div>
        </div>
    );
}

interface ButtonControlProps {
    button: ButtonDefinition;
    onChange: (label: string, pressed: boolean) => void;
}

function ButtonControl({ button, onChange }: ButtonControlProps) {
    const [held, setHeld] = useState(false);

    const press = useCallback(
        (pressed: boolean) => {
            setHeld(pressed);
            onChange(button.label, pressed);
        },
        [button.label, onChange],
    );

    if (button.mode === 'toggle') {
        return (
            <div className="button-control">
                <button
                    type="button"
                    aria-pressed={button.value}
                    className={`button-input button-toggle${button.value ? ' active' : ''}`}
                    onClick={() => onChange(button.label, !button.value)}
                >
                    <span className="button-label">{button.label}</span>
                    <span className="button-led" />
                </button>
            </div>
        );
    }

    // Momentary gate: pointer down raises the backing signal; pointer
    // up/leave/cancel lowers it so gates can't stick.
    return (
        <div className="button-control">
            <button
                type="button"
                className={`button-input button-gate${held ? ' active' : ''}`}
                onPointerDown={() => press(true)}
                onPointerUp={() => press(false)}
                onPointerLeave={() => held && press(false)}
                onPointerCancel={() => press(false)}
            >
                <span className="button-label">{button.label}</span>
                <span className="button-led" />
            </button>
        </div>
    );
}
