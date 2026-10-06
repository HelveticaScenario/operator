import { useCallback, useState } from 'react';
import type { ButtonView } from '../../app/controlBinding';
import {
    controlTooltip,
    jumpHandlers,
    unsyncedMessage,
    UnsyncedMarker,
} from './controlChrome';

interface ButtonControlProps {
    button: ButtonView;
    onChange: (callStart: number, pressed: boolean) => void;
    onJump: (offset: number) => void;
}

export function ButtonControl({
    button,
    onChange,
    onJump,
}: ButtonControlProps) {
    const [held, setHeld] = useState(false);

    const press = useCallback(
        (pressed: boolean) => {
            setHeld(pressed);
            onChange(button.callStart, pressed);
        },
        [button.callStart, onChange],
    );

    const message = unsyncedMessage(button);
    const tooltip = controlTooltip(message);
    const jump = jumpHandlers(button.argsStart, onJump);
    const title = (
        <span className="control-title">
            <span className="button-label">{button.label}</span>
            {message && <UnsyncedMarker message={message} />}
        </span>
    );

    if (button.mode === 'toggle') {
        return (
            <div className="button-control" {...jump}>
                <button
                    type="button"
                    aria-pressed={button.value}
                    aria-disabled={button.incomplete}
                    title={tooltip}
                    className={`button-input button-toggle${button.value ? ' active' : ''}`}
                    onClick={() =>
                        !button.incomplete &&
                        onChange(button.callStart, !button.value)
                    }
                >
                    {title}
                    <span className="button-led" />
                </button>
            </div>
        );
    }

    // Momentary gate: pointer down raises the backing signal; pointer
    // up/leave/cancel lowers it so gates can't stick. A gate not in the
    // running patch has no signal to raise and no literal to rewrite. Unusable
    // buttons are marked aria-disabled rather than disabled so hovering still
    // shows their tooltip (disabled buttons receive no mouse events).
    const live = button.moduleId !== null;
    return (
        <div className="button-control" {...jump}>
            <button
                type="button"
                aria-disabled={!live}
                title={tooltip}
                className={`button-input button-gate${held ? ' active' : ''}`}
                onPointerDown={() => live && press(true)}
                onPointerUp={() => press(false)}
                onPointerLeave={() => held && press(false)}
                onPointerCancel={() => press(false)}
            >
                {title}
                <span className="button-led" />
            </button>
        </div>
    );
}
