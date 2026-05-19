import React, { useEffect, useMemo, useRef, useState } from 'react';
import type { ModuleProfileSample } from '@modular/core';
import {
    createColumnHelper,
    flexRender,
    getCoreRowModel,
    getSortedRowModel,
    useReactTable,
    type SortingState,
} from '@tanstack/react-table';
import electronAPI from '../electronAPI';
import './ModuleProfile.css';

interface ModuleProfileProps {
    isOpen: boolean;
    onClose: () => void;
}

type Unit = 'ns' | 'pct';

interface DisplayRow {
    moduleId: string;
    mode: string;
    selfNsPerSample: number;
    paramsNsPerSample: number;
    totalNsPerSample: number;
    samplesProcessed: number;
}

function nsPerSample(ns: number, samples: number): number {
    return samples > 0 ? ns / samples : 0;
}

function formatValue(
    valueNsPerSample: number,
    unit: Unit,
    sampleRateHz: number,
    bufferSize: number,
): string {
    const v = valueNsPerSample;
    if (unit === 'pct') {
        if (sampleRateHz <= 0 || bufferSize <= 0) return '—';
        const callbackBudgetNs = (bufferSize * 1e9) / sampleRateHz;
        const moduleCallbackNs = v * bufferSize;
        const pct = (moduleCallbackNs / callbackBudgetNs) * 100;
        if (pct >= 10) return `${pct.toFixed(1)}%`;
        if (pct >= 0.01) return `${pct.toFixed(2)}%`;
        return pct === 0 ? '0%' : '<0.01%';
    }
    if (v >= 10000) return `${(v / 1000).toFixed(1)} µs`;
    if (v >= 100) return `${v.toFixed(0)} ns`;
    if (v === 0) return '0 ns';
    return `${v.toFixed(1)} ns`;
}

function formatBar(value: number, max: number): string {
    if (max <= 0) return '0';
    return `${Math.min(100, (value / max) * 100).toFixed(0)}%`;
}

const SAMPLE_RATE_OPTIONS = [
    { label: 'Every callback', value: 1 },
    { label: '1 in 4', value: 4 },
    { label: '1 in 16', value: 16 },
    { label: '1 in 64', value: 64 },
];

const columnHelper = createColumnHelper<DisplayRow>();

export function ModuleProfile({ isOpen, onClose }: ModuleProfileProps) {
    const [rows, setRows] = useState<ModuleProfileSample[] | null>(null);
    const [sorting, setSorting] = useState<SortingState>([
        { id: 'self', desc: true },
    ]);
    const [sampleRate, setSampleRate] = useState(16);
    const [unit, setUnit] = useState<Unit>('ns');
    const [sampleRateHz, setSampleRateHz] = useState(48000);
    const [bufferSize, setBufferSize] = useState(256);
    const panelRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (!isOpen) return;
        electronAPI.audio
            .getCurrentState()
            .then((s) => {
                setSampleRateHz(s.sampleRate);
                if (s.bufferSize && s.bufferSize > 0) {
                    setBufferSize(s.bufferSize);
                }
            })
            .catch(console.error);
    }, [isOpen]);

    useEffect(() => {
        if (!isOpen) return;
        let cancelled = false;
        let intervalId: ReturnType<typeof setInterval> | null = null;

        const poll = () => {
            electronAPI.synthesizer
                .getModuleProfile()
                .then((data) => {
                    if (!cancelled) setRows(data);
                })
                .catch(console.error);
        };

        // Sequence enable → set-rate → poll so a stale sample-rate update
        // can't land after the disable on a rapid open/close. The enable
        // refcounts on the Rust side (AudioState), so concurrent consumers
        // remain safe even if this effect re-runs.
        void (async () => {
            try {
                await electronAPI.synthesizer.setModuleProfilingEnabled(true);
                if (cancelled) {
                    await electronAPI.synthesizer.setModuleProfilingEnabled(
                        false,
                    );
                    return;
                }
                await electronAPI.synthesizer.setModuleProfilingSampleRate(
                    sampleRate,
                );
                if (cancelled) return;
                poll();
                intervalId = setInterval(poll, 1000);
            } catch (err) {
                console.error(err);
            }
        })();

        return () => {
            cancelled = true;
            if (intervalId !== null) clearInterval(intervalId);
            electronAPI.synthesizer
                .setModuleProfilingEnabled(false)
                .catch(console.error);
            setRows(null);
        };
    }, [isOpen, sampleRate]);

    useEffect(() => {
        if (!isOpen) return;
        const rafId = requestAnimationFrame(() => {
            panelRef.current?.focus();
        });
        return () => cancelAnimationFrame(rafId);
    }, [isOpen]);

    useEffect(() => {
        if (!isOpen) return;
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') onClose();
        };
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, onClose]);

    const data = useMemo<DisplayRow[]>(() => {
        if (!rows) return [];
        return rows.map((r) => {
            const self = nsPerSample(r.selfNs, r.samplesProcessed);
            const total = nsPerSample(r.totalNs, r.samplesProcessed);
            // Clamp to 0: clock granularity and timer skew between
            // push_frame and pop_frame can briefly push self past total.
            const params = Math.max(0, total - self);
            return {
                moduleId: r.moduleId,
                mode: r.mode,
                selfNsPerSample: self,
                paramsNsPerSample: params,
                totalNsPerSample: total,
                samplesProcessed: r.samplesProcessed,
            };
        });
    }, [rows]);

    const maxSelfNsPerSample = useMemo(() => {
        let m = 0;
        for (const r of data) {
            if (r.selfNsPerSample > m) m = r.selfNsPerSample;
        }
        return m;
    }, [data]);

    const columns = useMemo(
        () => [
            columnHelper.accessor('moduleId', {
                id: 'module',
                header: 'Module',
                cell: (info) => (
                    <span title={info.getValue()}>{info.getValue()}</span>
                ),
                sortingFn: (a, b) =>
                    a.original.moduleId.localeCompare(b.original.moduleId),
            }),
            columnHelper.accessor('mode', {
                id: 'mode',
                header: 'Mode',
                cell: (info) => {
                    const m = info.getValue();
                    return (
                        <span
                            className={`module-profile-mode module-profile-mode-${m}`}
                            title={
                                m === 'sample'
                                    ? 'Sample mode: in a feedback cycle, processes one sample per ensure_processed_to call. Higher wrapper overhead per sample.'
                                    : 'Block mode: acyclic subgraph, processes a full block per call. Lower per-sample overhead.'
                            }
                        >
                            {m}
                        </span>
                    );
                },
                sortingFn: (a, b) =>
                    a.original.mode.localeCompare(b.original.mode),
            }),
            columnHelper.accessor('selfNsPerSample', {
                id: 'self',
                header: 'Self',
                cell: (info) => {
                    const v = info.getValue();
                    return (
                        <div className="module-profile-cell">
                            <span className="module-profile-bar">
                                <span
                                    className="module-profile-bar-fill"
                                    style={{
                                        width: formatBar(
                                            v,
                                            maxSelfNsPerSample,
                                        ),
                                    }}
                                />
                            </span>
                            <span>
                                {formatValue(
                                    v,
                                    unit,
                                    sampleRateHz,
                                    bufferSize,
                                )}
                            </span>
                        </div>
                    );
                },
                sortingFn: (a, b) =>
                    a.original.selfNsPerSample - b.original.selfNsPerSample,
            }),
            columnHelper.accessor('paramsNsPerSample', {
                id: 'params',
                header: 'Params',
                cell: (info) =>
                    formatValue(
                        info.getValue(),
                        unit,
                        sampleRateHz,
                        bufferSize,
                    ),
                sortingFn: (a, b) =>
                    a.original.paramsNsPerSample -
                    b.original.paramsNsPerSample,
            }),
            columnHelper.accessor('totalNsPerSample', {
                id: 'total',
                header: 'Total',
                cell: (info) =>
                    formatValue(
                        info.getValue(),
                        unit,
                        sampleRateHz,
                        bufferSize,
                    ),
                sortingFn: (a, b) =>
                    a.original.totalNsPerSample - b.original.totalNsPerSample,
            }),
        ],
        [maxSelfNsPerSample, unit, sampleRateHz, bufferSize],
    );

    const table = useReactTable({
        data,
        columns,
        state: { sorting },
        onSortingChange: setSorting,
        getCoreRowModel: getCoreRowModel(),
        getSortedRowModel: getSortedRowModel(),
    });

    if (!isOpen) return null;

    return (
        <div className="module-profile-overlay" onClick={onClose}>
            <div
                className="module-profile-panel"
                ref={panelRef}
                tabIndex={-1}
                onClick={(e) => e.stopPropagation()}
            >
                <div className="module-profile-header">
                    <h2>Module Profile</h2>
                    <div className="module-profile-controls">
                        <div
                            className="module-profile-unit-toggle"
                            role="group"
                            aria-label="Display unit"
                        >
                            <button
                                type="button"
                                className={unit === 'ns' ? 'active' : ''}
                                onClick={() => setUnit('ns')}
                            >
                                ns
                            </button>
                            <button
                                type="button"
                                className={unit === 'pct' ? 'active' : ''}
                                onClick={() => setUnit('pct')}
                                title={`% of audio callback budget — ${bufferSize} frames at ${sampleRateHz} Hz = ${((bufferSize * 1e6) / sampleRateHz).toFixed(2)} ms per callback`}
                            >
                                %
                            </button>
                        </div>
                        <label className="module-profile-rate-label">
                            Sample
                            <select
                                className="module-profile-rate-select"
                                value={sampleRate}
                                onChange={(e) =>
                                    setSampleRate(Number(e.target.value))
                                }
                            >
                                {SAMPLE_RATE_OPTIONS.map((opt) => (
                                    <option key={opt.value} value={opt.value}>
                                        {opt.label}
                                    </option>
                                ))}
                            </select>
                        </label>
                        <button
                            className="module-profile-close-btn"
                            onClick={onClose}
                            aria-label="Close"
                        >
                            ×
                        </button>
                    </div>
                </div>

                <div className="module-profile-body">
                    <p className="module-profile-legend">
                        Per-sample averages.{' '}
                        <strong>Self</strong>: time in this module's own DSP.{' '}
                        <strong>Params</strong>: time fetching cable inputs
                        from upstream (their cost shows in their own Self).{' '}
                        <strong>Total</strong>: Self + Params.{' '}
                        <strong>Mode</strong>: <em>sample</em> = in a
                        feedback cycle (per-sample wrapper); <em>block</em> =
                        acyclic (per-block wrapper).
                    </p>
                    {rows === null ? (
                        <div className="module-profile-loading">Loading…</div>
                    ) : data.length === 0 ? (
                        <div className="module-profile-loading">
                            No module activity yet.
                        </div>
                    ) : (
                        <table className="module-profile-table">
                            <colgroup>
                                <col className="col-module" />
                                <col className="col-mode" />
                                <col className="col-self" />
                                <col className="col-params" />
                                <col className="col-total" />
                            </colgroup>
                            <thead>
                                {table.getHeaderGroups().map((hg) => (
                                    <tr key={hg.id}>
                                        {hg.headers.map((header) => {
                                            const isNumeric =
                                                header.column.id === 'self' ||
                                                header.column.id ===
                                                    'params' ||
                                                header.column.id === 'total';
                                            const sortDir =
                                                header.column.getIsSorted();
                                            return (
                                                <th
                                                    key={header.id}
                                                    onClick={header.column.getToggleSortingHandler()}
                                                    className={`${isNumeric ? 'numeric' : ''} ${sortDir ? 'active' : ''}`}
                                                >
                                                    {flexRender(
                                                        header.column.columnDef
                                                            .header,
                                                        header.getContext(),
                                                    )}
                                                    {sortDir === 'asc' && ' ▲'}
                                                    {sortDir === 'desc' &&
                                                        ' ▼'}
                                                </th>
                                            );
                                        })}
                                    </tr>
                                ))}
                            </thead>
                            <tbody>
                                {table.getRowModel().rows.map((row) => (
                                    <tr key={row.id}>
                                        {row.getVisibleCells().map((cell) => {
                                            const isNumeric =
                                                cell.column.id === 'self' ||
                                                cell.column.id === 'params' ||
                                                cell.column.id === 'total';
                                            return (
                                                <td
                                                    key={cell.id}
                                                    className={
                                                        isNumeric
                                                            ? 'numeric'
                                                            : ''
                                                    }
                                                >
                                                    {flexRender(
                                                        cell.column.columnDef
                                                            .cell,
                                                        cell.getContext(),
                                                    )}
                                                </td>
                                            );
                                        })}
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    )}
                </div>
            </div>
        </div>
    );
}
