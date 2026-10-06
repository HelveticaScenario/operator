import { useCallback, useState } from 'react';
import type { SliderView } from '../../app/controlBinding';
import {
    positionToVolts,
    sliderPositionCount,
    voltsToPosition,
} from '../../app/sliderPositions';
import { voltsToHz, voltsToNoteName } from '../../../shared/dsl/sliderUnits';
import {
    controlTooltip,
    jumpHandlers,
    unsyncedMessage,
    UnsyncedMarker,
} from './controlChrome';

interface SliderControlProps {
    slider: SliderView;
    onChange: (callStart: number, newValue: number) => void;
    onJump: (offset: number) => void;
}

export function SliderControl({
    slider,
    onChange,
    onJump,
}: SliderControlProps) {
    const [localValue, setLocalValue] = useState(slider.value);

    // Sync local state when slider definition changes (e.g., re-execution)
    const [prevValue, setPrevValue] = useState(slider.value);
    if (slider.value !== prevValue) {
        setLocalValue(slider.value);
        setPrevValue(slider.value);
    }

    const handleInput = useCallback(
        (e: React.ChangeEvent<HTMLInputElement>) => {
            const newValue = positionToVolts(
                parseInt(e.currentTarget.value, 10),
                slider,
            );
            setLocalValue(newValue);
            onChange(slider.callStart, newValue);
        },
        [slider, onChange],
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

    const message = unsyncedMessage(slider);

    return (
        <div
            className={`slider-control${slider.incomplete ? ' incomplete' : ''}`}
            title={controlTooltip(message)}
            {...jumpHandlers(slider.argsStart, onJump)}
        >
            <div className="slider-header">
                <span className="control-title">
                    <span className="slider-label">{slider.label}</span>
                    {message && <UnsyncedMarker message={message} />}
                </span>
                <span className="slider-value">{formatValue(localValue)}</span>
            </div>
            <input
                type="range"
                className="slider-input"
                min={0}
                max={sliderPositionCount(slider)}
                step={1}
                value={voltsToPosition(localValue, slider)}
                disabled={slider.incomplete}
                onChange={handleInput}
            />
            <div className="slider-range">
                <span>{formatValue(slider.min)}</span>
                <span>{formatValue(slider.max)}</span>
            </div>
        </div>
    );
}
