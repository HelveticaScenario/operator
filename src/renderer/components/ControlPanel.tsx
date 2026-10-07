import type { ButtonView, SliderView } from '../app/controlBinding';
import type { StaticGroup } from '../dsl/extractControls';
import { ButtonControl } from './controlPanel/ButtonControl';
import {
    controlTooltip,
    jumpHandlers,
    useJumpModifier,
} from './controlPanel/controlChrome';
import { SliderControl } from './controlPanel/SliderControl';
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
                        <code>$toggleBtn(label, initial)</code> in your patch.
                        Pass a <code>$cGroup(label)</code> as a control&apos;s
                        last argument to group it.
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

/** One group's contents: its controls in source order, then its subgroups. */
function ControlList({
    path,
    sliders,
    buttons,
    groups,
    handlers,
}: ControlListProps) {
    const controls = [
        ...sliders
            .filter((s) => inGroup(s, path))
            .map((slider) => ({ callStart: slider.callStart, slider })),
        ...buttons
            .filter((b) => inGroup(b, path))
            .map((button) => ({ button, callStart: button.callStart })),
    ].sort((a, b) => a.callStart - b.callStart);
    const ownGroups = groups.filter((g) => inGroup(g, path));
    return (
        <>
            {controls.length > 0 && (
                <div className="control-panel-controls">
                    {controls.map((c) =>
                        'slider' in c ? (
                            <SliderControl
                                key={`slider:${c.slider.label}`}
                                slider={c.slider}
                                onChange={handlers.onSliderChange}
                                onJump={handlers.onJump}
                            />
                        ) : (
                            <ButtonControl
                                key={`button:${c.button.label}`}
                                button={c.button}
                                onChange={handlers.onButtonChange}
                                onJump={handlers.onJump}
                            />
                        ),
                    )}
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
    const toggleable = group.collapsible;
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
