import { useCallback, useState } from 'react';
import type { SliderDefinition } from '../../shared/dsl/sliderTypes';
import './ControlPanel.css';

interface ControlPanelProps {
    sliders: SliderDefinition[];
    onSliderChange: (label: string, newValue: number) => void;
}

export function ControlPanel({ sliders, onSliderChange }: ControlPanelProps) {
    if (sliders.length === 0) {
        return (
            <div className="control-panel control-panel-empty">
                <div className="control-panel-placeholder">
                    <p>No sliders defined.</p>
                    <p className="control-panel-hint">
                        Use <code>$slider(label, value, min, max)</code> in your
                        patch.
                    </p>
                </div>
            </div>
        );
    }

    return (
        <div className="control-panel">
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

    const step = (slider.max - slider.min) / 1000;

    const handleInput = useCallback(
        (e: React.ChangeEvent<HTMLInputElement>) => {
            const newValue = parseFloat(e.target.value);
            setLocalValue(newValue);
            onChange(slider.label, newValue);
        },
        [slider.label, onChange],
    );

    const formatValue = (v: number): string =>
        Number(v.toPrecision(4)).toString();

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
                onInput={handleInput}
            />
            <div className="slider-range">
                <span>{formatValue(slider.min)}</span>
                <span>{formatValue(slider.max)}</span>
            </div>
        </div>
    );
}
