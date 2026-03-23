import { useRef } from 'react';
import { useNavigate, useParams } from '@tanstack/react-router';
import { Plus } from 'lucide-react';
import { useQuery, useMutation } from '@tanstack/react-query';
import { fetchTasks, createTask } from '../../api/client';
import { cn, getStatusDot, getStatusBgColor } from '../../lib/utils';
import type { TaskInfo } from '../../types';

interface TaskCardProps {
    task: TaskInfo;
    isActive: boolean;
    onClick: () => void;
}

function TaskCard({ task, isActive, onClick }: TaskCardProps) {
    return (
        <button
            onClick={onClick}
            className={cn(
                'w-full text-left px-4 py-3 transition-all',
                'border-b border-zinc-800/60 last:border-0',
                isActive ? 'bg-zinc-800' : 'hover:bg-zinc-800/50 active:bg-zinc-800',
            )}
        >
            <div className="flex items-center gap-2.5 min-w-0">
                <span className={cn('w-2 h-2 rounded-full shrink-0', getStatusDot(task.state), task.state === 'Running' && 'animate-pulse-dot')} />
                <span className="text-sm font-medium text-zinc-200 truncate flex-1">{task.name}</span>
                <span className={cn('text-[10px] font-medium px-1.5 py-0.5 rounded-full ring-1 ring-inset shrink-0', getStatusBgColor(task.state))}>
                    {task.state}
                </span>
            </div>
            <div className="mt-1 ml-4.5">
                <span className="text-[11px] text-zinc-500 font-mono">{task.profile}</span>
            </div>
        </button>
    );
}

interface SidebarProps {
    onNavigate: () => void;
}

export function Sidebar({ onNavigate }: SidebarProps) {
    const params = useParams({ strict: false });
    const activeId = (params as Record<string, string>).id;
    const navigate = useNavigate();
    const onNavigateRef = useRef(onNavigate);
    onNavigateRef.current = onNavigate;

    const { data: tasksData } = useQuery({
        queryKey: ['tasks'],
        queryFn: fetchTasks,
        staleTime: 0,
    });

    const { data: connected = false } = useQuery<boolean>({ queryKey: ['ws_connected'], initialData: false, staleTime: Infinity });

    const sortedTasks = [...(tasksData?.tasks ?? [])].sort(
        (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
    );

    const createMutation = useMutation({
        mutationFn: createTask,
        onSuccess: () => {
            // New task will appear via WebSocket TaskCreated event
            onNavigateRef.current();
        },
    });

    const handleNew = () => {
        if (!connected || createMutation.isPending) return;
        createMutation.mutate({});
    };

    const handleTaskClick = (id: string) => {
        void navigate({ to: '/session/$id', params: { id } });
        onNavigate();
    };

    return (
        <div className="flex flex-col h-full overflow-hidden">
            <div className="flex items-center justify-between px-4 py-3 border-b border-zinc-800 shrink-0 bg-zinc-900 sticky top-0 z-10">
                <span className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">Tasks</span>
                <button
                    onClick={handleNew}
                    disabled={!connected || createMutation.isPending}
                    className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg bg-zinc-800 text-zinc-300 hover:bg-zinc-700 hover:text-zinc-100 transition-colors border border-zinc-700/50 disabled:opacity-40 disabled:cursor-not-allowed font-medium"
                    title={connected ? 'New task' : 'Not connected'}
                >
                    <Plus size={13} />
                    New
                </button>
            </div>

            <div className="flex-1 overflow-y-auto flex flex-col">
                {sortedTasks.length === 0 ? (
                    <div className="flex flex-col items-center justify-center flex-1 gap-2 text-zinc-600 py-16">
                        <span className="text-sm">No tasks yet</span>
                        <span className="text-xs text-zinc-700">
                            {connected ? 'Tap New to get started' : 'Connecting...'}
                        </span>
                    </div>
                ) : (
                    sortedTasks.map((task) => (
                        <TaskCard
                            key={task.id}
                            task={task}
                            isActive={task.id === activeId}
                            onClick={() => handleTaskClick(task.id)}
                        />
                    ))
                )}
            </div>
        </div>
    );
}
