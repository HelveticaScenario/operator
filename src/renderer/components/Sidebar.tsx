import { type ReactNode, useState } from 'react';
import './Sidebar.css';

export type SidebarTab = 'explorer' | 'control' | 'perform';

const TABS: { id: SidebarTab; label: string }[] = [
    { id: 'explorer', label: 'Explorer' },
    { id: 'control', label: 'Control' },
    { id: 'perform', label: 'Perform' },
];

interface SidebarProps {
    explorerContent: ReactNode;
    controlContent: ReactNode;
    performContent: ReactNode;
}

export function Sidebar({
    explorerContent,
    controlContent,
    performContent,
}: SidebarProps) {
    const [activeTab, setActiveTab] = useState<SidebarTab>('explorer');

    return (
        <div className="app-sidebar">
            <div className="app-sidebar-tabs">
                {TABS.map(({ id, label }) => (
                    <button
                        key={id}
                        className={`app-sidebar-tab ${activeTab === id ? 'active' : ''}`}
                        onClick={() => setActiveTab(id)}
                    >
                        {label}
                    </button>
                ))}
            </div>
            <div className="app-sidebar-content">
                {
                    {
                        control: controlContent,
                        explorer: explorerContent,
                        perform: performContent,
                    }[activeTab]
                }
            </div>
        </div>
    );
}
