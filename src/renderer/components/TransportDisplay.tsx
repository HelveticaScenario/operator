import type { TransportSnapshot } from '../../shared/ipcTypes';

interface TransportDisplayProps {
    transport: TransportSnapshot | null;
    onToggleLink?: (enabled: boolean) => void;
}

export function TransportDisplay({
    transport,
    onToggleLink,
}: TransportDisplayProps) {
    if (!transport) {
        return (
            <div className="transport-display">
                <span className="transport-position transport-dim">---.--</span>
            </div>
        );
    }

    const {
        bpm,
        timeSigNumerator,
        timeSigDenominator,
        bar,
        beatInBar,
        hasQueuedUpdate,
        linkPendingStart,
    } = transport;

    // Display bar as 1-indexed
    const displayBar = bar + 1;
    // Beat is 0-indexed in the data, display as 1-indexed
    const displayBeat = beatInBar + 1;

    // Beat indicator pips
    const pips: boolean[] = [];
    for (let i = 0; i < timeSigNumerator; i++) {
        pips.push(i === beatInBar);
    }

    return (
        <div className="transport-display">
            {/* Tempo */}
            <span className="transport-tempo" title="Tempo (BPM)">
                {bpm.toFixed(0)}
            </span>

            {/* Time signature */}
            <span
                className="transport-timesig transport-dim"
                title="Time signature"
            >
                {timeSigNumerator}/{timeSigDenominator}
            </span>

            {/* Bar.Beat position */}
            <span className="transport-position" title="Bar.Beat">
                {displayBar}.{displayBeat}
            </span>

            {/* Beat pips */}
            <span className="transport-pips" title="Beat position">
                {pips.map((active, i) => (
                    <span
                        key={i}
                        className={`transport-pip${active ? ' active' : ''}`}
                    />
                ))}
            </span>

            {/* Link toggle with phase indicator */}
            <button
                className={`transport-link${transport.linkEnabled ? ' active' : ''}`}
                onClick={() => onToggleLink?.(!transport.linkEnabled)}
                title={
                    transport.linkEnabled
                        ? `Link active (${transport.linkPeers} peer${transport.linkPeers !== 1 ? 's' : ''})`
                        : 'Enable Ableton Link'
                }
            >
                {transport.linkEnabled && (
                    <span
                        className="transport-link-phase"
                        style={{
                            width: `${(transport.linkPhase ?? 0) * 100}%`,
                        }}
                    />
                )}
                {/* Two layered labels with clip-path masks for split text color */}
                {transport.linkEnabled ? (
                    <>
                        {/* Invisible copy in normal flow to maintain button size */}
                        <span
                            className="transport-link-label"
                            aria-hidden="true"
                            style={{ visibility: 'hidden' }}
                        >
                            Link
                            {transport.linkPeers > 0 && (
                                <span className="transport-link-peers">
                                    {transport.linkPeers}
                                </span>
                            )}
                        </span>
                        {/* Dark text over the filled region */}
                        <span
                            className="transport-link-label filled"
                            style={{
                                clipPath: `inset(0 ${100 - (transport.linkPhase ?? 0) * 100}% 0 0)`,
                            }}
                        >
                            Link
                            {transport.linkPeers > 0 && (
                                <span className="transport-link-peers">
                                    {transport.linkPeers}
                                </span>
                            )}
                        </span>
                        {/* Accent text over the unfilled region */}
                        <span
                            className="transport-link-label unfilled"
                            style={{
                                clipPath: `inset(0 0 0 ${(transport.linkPhase ?? 0) * 100}%)`,
                            }}
                        >
                            Link
                            {transport.linkPeers > 0 && (
                                <span className="transport-link-peers">
                                    {transport.linkPeers}
                                </span>
                            )}
                        </span>
                    </>
                ) : (
                    <span className="transport-link-label">Link</span>
                )}
            </button>

            {/* Armed-start indicator: start requested, waiting for next Link bar */}
            {linkPendingStart && (
                <span
                    className="transport-armed"
                    title="Waiting for Link bar boundary to start"
                >
                    ⧗
                </span>
            )}

            {/* Queued update indicator */}
            {hasQueuedUpdate && (
                <span className="transport-queued" title="Update queued">
                    Q
                </span>
            )}
        </div>
    );
}
