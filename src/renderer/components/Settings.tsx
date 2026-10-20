import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTheme } from '../themes/ThemeContext';
import { type AudioSettingsHandle, AudioSettingsTab } from './AudioSettings';
import { EditorSettingsTab } from './EditorSettingsTab';
import { FormatterSettingsTab } from './FormatterSettingsTab';
import { bindSettingsModal } from '../keybindings/contextKeyBootstrap';
import { useEditableConfig } from '../hooks/useEditableConfig';
import './Settings.css';

type SettingsTabId = 'editor' | 'audio' | 'formatter';

interface SettingsTab {
    id: SettingsTabId;
    label: string;
}

const TABS: SettingsTab[] = [
    { id: 'editor', label: 'Editor' },
    { id: 'audio', label: 'Audio' },
    { id: 'formatter', label: 'Formatter' },
];

interface SettingsProps {
    isOpen: boolean;
    onClose: () => void;
}

export function Settings({ isOpen, onClose }: SettingsProps) {
    const [activeTab, setActiveTab] = useState<SettingsTabId>('editor');
    const [draftConfig, handleDraftChange] = useEditableConfig(isOpen);
    const [saving, setSaving] = useState(false);
    const { themes } = useTheme();

    const audioRef = useRef<AudioSettingsHandle>(null);
    const panelRef = useRef<HTMLDivElement>(null);

    const handleAudioSave = useCallback(async () => {
        setSaving(true);
        try {
            // Apply audio device changes
            await audioRef.current?.apply();
            onClose();
        } catch (err) {
            console.error('Failed to apply audio settings:', err);
        } finally {
            setSaving(false);
        }
    }, [onClose]);

    // Close on Escape key
    useEffect(() => {
        if (!isOpen) {
            return;
        }
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                onClose();
            }
        };
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, onClose]);

    // Focus the panel when opened so the editor loses focus
    useEffect(() => {
        if (isOpen) {
            // Use rAF to ensure the DOM has rendered before focusing
            requestAnimationFrame(() => {
                panelRef.current?.focus();
            });
        }
    }, [isOpen]);

    // Mirror modal state into the context-key service for when-clauses.
    useEffect(() => bindSettingsModal(isOpen), [isOpen]);

    if (!isOpen) {
        return null;
    }

    return (
        <div className="settings-overlay" onClick={onClose}>
            <div
                className="settings-panel"
                ref={panelRef}
                tabIndex={-1}
                onClick={(e) => e.stopPropagation()}
            >
                <div className="settings-header">
                    <h2>Settings</h2>
                    <button className="close-btn" onClick={onClose}>
                        ×
                    </button>
                </div>

                <div className="settings-tabs">
                    {TABS.map((tab) => (
                        <button
                            key={tab.id}
                            className={`settings-tab-btn${activeTab === tab.id ? ' active' : ''}`}
                            onClick={() => setActiveTab(tab.id)}
                        >
                            {tab.label}
                        </button>
                    ))}
                </div>

                <div className="settings-body">
                    {activeTab === 'editor' && (
                        <EditorSettingsTab
                            config={draftConfig}
                            themes={themes}
                            onConfigChange={handleDraftChange}
                        />
                    )}
                    {activeTab === 'audio' && (
                        <AudioSettingsTab
                            isActive={activeTab === 'audio'}
                            ref={audioRef}
                        />
                    )}
                    {activeTab === 'formatter' && (
                        <FormatterSettingsTab
                            config={draftConfig}
                            onConfigChange={handleDraftChange}
                        />
                    )}
                </div>

                {activeTab === 'audio' && (
                    <div className="settings-footer">
                        <button
                            className="btn btn-primary"
                            onClick={handleAudioSave}
                            disabled={saving}
                        >
                            {saving ? 'Saving...' : 'Save'}
                        </button>
                    </div>
                )}
            </div>
        </div>
    );
}
