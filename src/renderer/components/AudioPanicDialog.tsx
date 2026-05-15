import React, { useEffect, useState } from 'react';
import electronAPI from '../electronAPI';
import './AudioPanicDialog.css';

const POLL_INTERVAL_MS = 500;

export function AudioPanicDialog() {
    const [panicked, setPanicked] = useState(false);
    const [restarting, setRestarting] = useState(false);
    const [restartError, setRestartError] = useState<string | null>(null);
    const [logDir, setLogDir] = useState<string | null>(null);

    useEffect(() => {
        let cancelled = false;
        const poll = () => {
            electronAPI.synthesizer
                .isAudioThreadPanicked()
                .then((p) => {
                    if (!cancelled) setPanicked(p);
                })
                .catch(() => {
                    /* transient IPC errors are ignored — next poll retries */
                });
        };
        poll();
        const id = setInterval(poll, POLL_INTERVAL_MS);
        return () => {
            cancelled = true;
            clearInterval(id);
        };
    }, []);

    useEffect(() => {
        let cancelled = false;
        electronAPI.synthesizer
            .panicLogDir()
            .then((dir) => {
                if (!cancelled) setLogDir(dir);
            })
            .catch(() => {
                /* fall back to no path shown */
            });
        return () => {
            cancelled = true;
        };
    }, []);

    const handleRestart = async () => {
        setRestarting(true);
        setRestartError(null);
        try {
            await electronAPI.synthesizer.restartAudio();
            const stillPanicked =
                await electronAPI.synthesizer.isAudioThreadPanicked();
            setPanicked(stillPanicked);
        } catch (e) {
            setRestartError(e instanceof Error ? e.message : String(e));
        } finally {
            setRestarting(false);
        }
    };

    if (!panicked) return null;

    return (
        <div className="audio-panic-overlay">
            <div
                className="audio-panic-panel"
                role="alertdialog"
                aria-modal="true"
                aria-labelledby="audio-panic-title"
            >
                <h2 id="audio-panic-title">Audio engine crashed</h2>
                <p>
                    The audio thread hit an unrecoverable error and is now
                    silent.
                    {logDir && (
                        <>
                            {' '}
                            A panic log was written to{' '}
                            <button
                                type="button"
                                className="audio-panic-log-link"
                                onClick={() => {
                                    void electronAPI.shell.openPath(logDir);
                                }}
                                title={`Open ${logDir} in your file manager`}
                            >
                                <code>{logDir}</code>
                            </button>
                            .
                        </>
                    )}
                </p>
                <p>Restart the audio engine to resume playback.</p>
                {restartError && (
                    <div className="audio-panic-error">
                        Restart failed: {restartError}
                    </div>
                )}
                <div className="audio-panic-actions">
                    <button
                        className="audio-panic-restart-btn"
                        onClick={handleRestart}
                        disabled={restarting}
                        autoFocus
                    >
                        {restarting ? 'Restarting…' : 'Restart Audio'}
                    </button>
                </div>
            </div>
        </div>
    );
}
