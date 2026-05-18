import * as Tabs from '@radix-ui/react-tabs';
import {
    DndContext,
    PointerSensor,
    closestCenter,
    useSensor,
    useSensors,
    type DragEndEvent,
} from '@dnd-kit/core';
import {
    SortableContext,
    arrayMove,
    horizontalListSortingStrategy,
    useSortable,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { getBufferId } from '../app/buffers';
import type { EditorBuffer } from '../types/editor';
import './EditorTabs.css';

export interface EditorTabsProps {
    buffers: EditorBuffer[];
    activeBufferId?: string;
    runningBufferId?: string | null;
    formatLabel: (buffer: EditorBuffer) => string;
    onSelectBuffer: (id: string) => void;
    onCloseBuffer: (id: string) => void;
    onReorderBuffers: (next: EditorBuffer[]) => void;
}

interface SortableTabProps {
    buffer: EditorBuffer;
    isActive: boolean;
    isRunning: boolean;
    label: string;
    onClose: (id: string) => void;
}

function SortableTab({
    buffer,
    isActive,
    isRunning,
    label,
    onClose,
}: SortableTabProps) {
    const id = getBufferId(buffer);
    const {
        attributes,
        listeners,
        setNodeRef,
        transform,
        transition,
        isDragging,
    } = useSortable({ id });

    const style: React.CSSProperties = {
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isDragging ? 0.5 : 1,
        zIndex: isDragging ? 1 : undefined,
    };

    const handleMouseDown = (e: React.MouseEvent) => {
        if (e.button === 1) {
            e.preventDefault();
            onClose(id);
        }
    };

    const className = [
        'editor-tab',
        isActive && 'active',
        buffer.dirty && 'dirty',
        isRunning && 'running',
        buffer.isPreview && 'preview',
        isDragging && 'dragging',
    ]
        .filter(Boolean)
        .join(' ');

    return (
        <Tabs.Trigger value={id} asChild>
            <div
                ref={setNodeRef}
                style={style}
                className={className}
                onMouseDown={handleMouseDown}
                {...attributes}
                {...listeners}
            >
                <span className="editor-tab-label">{label}</span>
                {isRunning && <span className="running-badge">▶</span>}
                {buffer.dirty && <span className="dirty-dot">●</span>}
                <button
                    type="button"
                    className="editor-tab-close"
                    aria-label="Close tab"
                    title="Close"
                    onPointerDown={(e) => e.stopPropagation()}
                    onClick={(e) => {
                        e.stopPropagation();
                        onClose(id);
                    }}
                >
                    ×
                </button>
            </div>
        </Tabs.Trigger>
    );
}

export function EditorTabs({
    buffers,
    activeBufferId,
    runningBufferId,
    formatLabel,
    onSelectBuffer,
    onCloseBuffer,
    onReorderBuffers,
}: EditorTabsProps) {
    const sensors = useSensors(
        useSensor(PointerSensor, {
            activationConstraint: { distance: 4 },
        }),
    );

    const handleDragEnd = (event: DragEndEvent) => {
        const { active, over } = event;
        if (!over || active.id === over.id) {
            return;
        }
        const oldIndex = buffers.findIndex((b) => getBufferId(b) === active.id);
        const newIndex = buffers.findIndex((b) => getBufferId(b) === over.id);
        if (oldIndex === -1 || newIndex === -1) {
            return;
        }
        onReorderBuffers(arrayMove(buffers, oldIndex, newIndex));
    };

    if (buffers.length === 0) {
        return null;
    }

    const itemIds = buffers.map((b) => getBufferId(b));

    return (
        <Tabs.Root
            className="editor-tabs-root"
            value={activeBufferId ?? ''}
            onValueChange={onSelectBuffer}
        >
            <DndContext
                sensors={sensors}
                collisionDetection={closestCenter}
                onDragEnd={handleDragEnd}
            >
                <SortableContext
                    items={itemIds}
                    strategy={horizontalListSortingStrategy}
                >
                    <Tabs.List className="editor-tabs" aria-label="Open buffers">
                        {buffers.map((buffer) => {
                            const id = getBufferId(buffer);
                            return (
                                <SortableTab
                                    key={id}
                                    buffer={buffer}
                                    isActive={id === activeBufferId}
                                    isRunning={id === runningBufferId}
                                    label={formatLabel(buffer)}
                                    onClose={onCloseBuffer}
                                />
                            );
                        })}
                    </Tabs.List>
                </SortableContext>
            </DndContext>
        </Tabs.Root>
    );
}
