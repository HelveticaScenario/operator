import { useCallback, useEffect, useState } from 'react';
import type {
    ButtonView,
    ControlBinding,
    SliderView,
} from '../app/controlBinding';
import type { StaticGroup } from '../dsl/extractControls';
import {
    snapVoltsToSemitone,
    voltsToHz,
    voltsToNoteName,
} from '../../shared/dsl/sliderUnits';
import './ControlPanel.css';

/** Controls and groups are identified by their call's offset (`callStart`):
 *  labels repeat across groups. */
interface ControlPanelProps {
    sliders: SliderView[];
    buttons: ButtonView[];
    groups: StaticGroup[];
    onSliderChange: (callStart: number, newValue: number) => void;
    onButtonChange: (callStart: number, pressed: boolean) => void;
    onGroupToggle: (callStart: number, collapsed: boolean) => void;
    /** Move the editor cursor to a source offset */
    onJump: (offset: number) => void;
}

type ListHandlers = Pick<
    ControlPanelProps,
    'onSliderChange' | 'onButtonChange' | 'onGroupToggle' | 'onJump'
>;

function inGroup(item: { group: string[] }, path: string[]): boolean {
    return (
        item.group.length === path.length &&
        item.group.every((label, i) => label === path[i])
    );
}

const JUMP_HINT = '⌃/⌘-click: go to code';

function isJumpGesture(e: React.MouseEvent | React.PointerEvent): boolean {
    return e.ctrlKey || e.metaKey;
}

/**
 * A Ctrl/Cmd-click on a control moves the editor cursor inside its call
 * instead of operating the control. The handlers run in the capture phase so
 * the control never sees the gesture; on macOS Ctrl+click also raises a
 * context menu, which is suppressed.
 */
function jumpHandlers(offset: number, onJump: (offset: number) => void) {
    return {
        onClickCapture: (e: React.MouseEvent) => {
            if (isJumpGesture(e)) {
                e.preventDefault();
                e.stopPropagation();
            }
        },
        onContextMenu: (e: React.MouseEvent) => {
            if (e.ctrlKey) {
                e.preventDefault();
            }
        },
        onMouseDownCapture: (e: React.MouseEvent) => {
            if (isJumpGesture(e)) {
                e.preventDefault();
                e.stopPropagation();
            }
        },
        onPointerDownCapture: (e: React.PointerEvent) => {
            if (isJumpGesture(e)) {
                e.preventDefault();
                e.stopPropagation();
                onJump(offset);
            }
        },
    };
}

/**
 * Whether Ctrl/Cmd is held, so the hovered control can show it is a jump
 * target. Window listeners run in the capture phase so the editor, which
 * usually has focus, cannot swallow the keys; losing focus clears the state
 * since the key-up may land in another window.
 */
function useJumpModifier(): [boolean, (held: boolean) => void] {
    const [held, setHeld] = useState(false);
    useEffect(() => {
        const onKey = (e: KeyboardEvent) => setHeld(e.ctrlKey || e.metaKey);
        const onBlur = () => setHeld(false);
        window.addEventListener('keydown', onKey, true);
        window.addEventListener('keyup', onKey, true);
        window.addEventListener('blur', onBlur);
        return () => {
            window.removeEventListener('keydown', onKey, true);
            window.removeEventListener('keyup', onKey, true);
            window.removeEventListener('blur', onBlur);
        };
    }, []);
    return [held, setHeld];
}

/** Tooltip for a control: why it is out of sync, if it is, plus the jump hint. */
function controlTooltip(message: string | undefined): string {
    return message ? `${message}\n${JUMP_HINT}` : JUMP_HINT;
}

export function ControlPanel({
    sliders,
    buttons,
    groups,
    ...handlers
}: ControlPanelProps) {
    const [jumpArmed, setJumpArmed] = useJumpModifier();

    if (sliders.length === 0 && buttons.length === 0 && groups.length === 0) {
        return (
            <div className="control-panel control-panel-empty">
                <div className="control-panel-placeholder">
                    <p>No controls defined.</p>
                    <p className="control-panel-hint">
                        Use <code>$slider(label, value, min, max)</code>,{' '}
                        <code>$btn(label)</code>, or{' '}
                        <code>$toggleBtn(label, initial)</code> in your patch,
                        and <code>$cGroup(label)</code> to group them.
                    </p>
                </div>
            </div>
        );
    }

    return (
        <div
            className={`control-panel${jumpArmed ? ' jump-armed' : ''}`}
            // Picks up a modifier pressed while another window had focus.
            onPointerMove={(e) => setJumpArmed(e.ctrlKey || e.metaKey)}
        >
            <ControlList
                path={[]}
                sliders={sliders}
                buttons={buttons}
                groups={groups}
                handlers={handlers}
            />
        </div>
    );
}

interface ControlListProps {
    /** The group whose direct contents to render; [] for the top level */
    path: string[];
    sliders: SliderView[];
    buttons: ButtonView[];
    groups: StaticGroup[];
    handlers: ListHandlers;
}

/** One group's contents: its buttons, then its sliders, then its subgroups. */
function ControlList({
    path,
    sliders,
    buttons,
    groups,
    handlers,
}: ControlListProps) {
    const ownButtons = buttons.filter((b) => inGroup(b, path));
    const ownSliders = sliders.filter((s) => inGroup(s, path));
    const ownGroups = groups.filter((g) => inGroup(g, path));
    return (
        <>
            {ownButtons.length > 0 && (
                <div className="control-panel-buttons">
                    {ownButtons.map((b) => (
                        <ButtonControl
                            key={b.label}
                            button={b}
                            onChange={handlers.onButtonChange}
                            onJump={handlers.onJump}
                        />
                    ))}
                </div>
            )}
            {ownSliders.length > 0 && (
                <div className="control-panel-sliders">
                    {ownSliders.map((s) => (
                        <SliderControl
                            key={s.label}
                            slider={s}
                            onChange={handlers.onSliderChange}
                            onJump={handlers.onJump}
                        />
                    ))}
                </div>
            )}
            {ownGroups.map((g) => (
                <GroupControl
                    key={g.label}
                    group={g}
                    sliders={sliders}
                    buttons={buttons}
                    groups={groups}
                    handlers={handlers}
                />
            ))}
        </>
    );
}

interface GroupControlProps {
    group: StaticGroup;
    sliders: SliderView[];
    buttons: ButtonView[];
    groups: StaticGroup[];
    handlers: ListHandlers;
}

/**
 * A collapsible group. Clicking the header writes the new collapsed state
 * into the `$cGroup` call; a call whose arguments are invalid cannot be
 * rewritten, so its header does not toggle.
 */
function GroupControl({
    group,
    sliders,
    buttons,
    groups,
    handlers,
}: GroupControlProps) {
    const toggleable = group.collapseEdit !== null;
    return (
        <div className={`control-group${group.collapsed ? ' collapsed' : ''}`}>
            <div
                className="control-group-header-wrap"
                {...jumpHandlers(group.argsStart, handlers.onJump)}
            >
                <button
                    type="button"
                    className="control-group-header"
                    aria-expanded={!group.collapsed}
                    aria-disabled={!toggleable}
                    title={controlTooltip(
                        toggleable
                            ? undefined
                            : 'Arguments are invalid — fix the $cGroup call to collapse this group',
                    )}
                    onClick={() =>
                        toggleable &&
                        handlers.onGroupToggle(
                            group.callStart,
                            !group.collapsed,
                        )
                    }
                >
                    <span className="control-group-chevron" aria-hidden />
                    <span className="control-group-label">{group.label}</span>
                </button>
            </div>
            {!group.collapsed && (
                <div className="control-group-body">
                    <ControlList
                        path={[...group.group, group.label]}
                        sliders={sliders}
                        buttons={buttons}
                        groups={groups}
                        handlers={handlers}
                    />
                </div>
            )}
        </div>
    );
}

/** Why a control does not match the running patch, or undefined if it does. */
function unsyncedMessage(control: ControlBinding): string | undefined {
    if (control.incomplete) {
        return 'Arguments are incomplete — finish editing to use this control';
    }
    if (control.synced) {
        return undefined;
    }
    return control.moduleId !== null
        ? 'Differs from the running patch — evaluate to apply'
        : 'Not in the running patch — evaluate to apply';
}

/** Shown on a control the running patch does not match — one not yet
 *  evaluated, in a buffer that is not running, edited since evaluation, or
 *  with arguments mid-edit. */
function UnsyncedMarker({ message }: { message: string }) {
    return <span className="control-unsynced" title={message} />;
}

interface SliderControlProps {
    slider: SliderView;
    onChange: (callStart: number, newValue: number) => void;
    onJump: (offset: number) => void;
}

function SliderControl({ slider, onChange, onJump }: SliderControlProps) {
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
            onChange(slider.callStart, newValue);
        },
        [slider.callStart, slider.unit, onChange],
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
                min={slider.min}
                max={slider.max}
                step={step}
                value={localValue}
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

interface ButtonControlProps {
    button: ButtonView;
    onChange: (callStart: number, pressed: boolean) => void;
    onJump: (offset: number) => void;
}

function ButtonControl({ button, onChange, onJump }: ButtonControlProps) {
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
