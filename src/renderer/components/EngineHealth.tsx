import React, { useEffect, useRef, useState } from 'react';
import type { AudioBudgetSnapshot } from '@modular/core';
import electronAPI from '../electronAPI';
import './EngineHealth.css';

interface EngineHealthProps {
    isOpen: boolean;
    onClose: () => void;
}

function usageClass(usage: number): string {
    if (usage >= 0.8) return 'danger';
    if (usage >= 0.5) return 'warning';
    return '';
}

function formatUsage(usage: number): string {
    return `${(usage * 100).toFixed(1)}%`;
}

function formatNs(ns: number): string {
    return `${ns.toFixed(0)} ns/sample`;
}

export function EngineHealth({ isOpen, onClose }: EngineHealthProps) {
    const [snapshot, setSnapshot] = useState<AudioBudgetSnapshot | null>(null);
    const panelRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (!isOpen) return;

        let cancelled = false;

        const poll = () => {
            electronAPI.synthesizer
                .getHealth()
                .then((data) => {
                    if (!cancelled) setSnapshot(data);
                })
                .catch(console.error);
        };

        poll();
        const intervalId = setInterval(poll, 1000);

        return () => {
            cancelled = true;
            clearInterval(intervalId);
            // Reset snapshot so next open shows "Loading…" instead of stale data.
            setSnapshot(null);
        };
    }, [isOpen]);

    // Focus panel when opened
    useEffect(() => {
        if (!isOpen) return;
        const rafId = requestAnimationFrame(() => {
            panelRef.current?.focus();
        });
        return () => cancelAnimationFrame(rafId);
    }, [isOpen]);

    // Close on Escape
    useEffect(() => {
        if (!isOpen) return;
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') onClose();
        };
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, onClose]);

    if (!isOpen) return null;

    return (
        <div className="engine-health-overlay" onClick={onClose}>
            <div
                className="engine-health-panel"
                ref={panelRef}
                tabIndex={-1}
                onClick={(e) => e.stopPropagation()}
            >
                <div className="engine-health-header">
                    <h2>Engine Health</h2>
                    <button
                        className="engine-health-close-btn"
                        onClick={onClose}
                        aria-label="Close"
                    >
                        ×
                    </button>
                </div>

                <div className="engine-health-body">
                    {snapshot === null ? (
                        <div className="engine-health-loading">Loading…</div>
                    ) : (
                        <div className="engine-health-section">
                            <p className="engine-health-section-title">
                                Audio CPU
                            </p>
                            <div className="engine-health-row">
                                <span className="engine-health-label">
                                    Average
                                </span>
                                <div className="engine-health-values">
                                    <span
                                        className={`engine-health-usage ${usageClass(snapshot.avgUsage)}`}
                                    >
                                        {formatUsage(snapshot.avgUsage)}
                                    </span>
                                    <span className="engine-health-ns">
                                        {formatNs(snapshot.avgNsPerSample)}
                                    </span>
                                </div>
                            </div>
                            <div className="engine-health-row">
                                <span className="engine-health-label">
                                    Peak
                                </span>
                                <div className="engine-health-values">
                                    <span
                                        className={`engine-health-usage ${usageClass(snapshot.peakUsage)}`}
                                    >
                                        {formatUsage(snapshot.peakUsage)}
                                    </span>
                                    <span className="engine-health-ns">
                                        {formatNs(snapshot.peakNsPerSample)}
                                    </span>
                                </div>
                            </div>
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
}
