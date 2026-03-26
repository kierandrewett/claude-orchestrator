import { useNavigate, useParams } from '@tanstack/react-router';
import { ArrowLeft, Square } from 'lucide-react';
import { useQuery, useMutation } from '@tanstack/react-query';
import { stopTask } from '../../api/client';
import { cn, getStatusBgColor, getStatusDot } from '../../lib/utils';
import { StatsPanel } from './StatsPanel';
import { EventStream } from './EventStream';
import { InputBar } from './InputBar';
import { trpc } from '../../api/trpc';
import type { OrchestratorEvent } from '../../types';

export function SessionViewer() {
    const params = useParams({ strict: false }) as { id?: string };
    const id = params.id ?? '';
    const navigate = useNavigate();

    // Use tRPC for task list
    const { data: tasks } = trpc.tasks.list.useQuery(undefined, { refetchInterval: 3000 });
    const task = tasks?.find((t) => t.id === id);

    const { data: events } = useQuery<OrchestratorEvent[]>({
        queryKey: ['history', id],
        staleTime: Infinity,
        enabled: !!id,
    });
    const taskEvents: OrchestratorEvent[] = events ?? [];

    const stopMutation = useMutation({ mutationFn: () => stopTask(id) });

    if (!task) {
        return (
            <div className="flex flex-col items-center justify-center h-full gap-3 text-zinc-600">
                <p className="text-sm">Task not found.</p>
                <button
                    onClick={() => void navigate({ to: '/tasks' })}
                    className="text-sm text-zinc-500 hover:text-zinc-300 flex items-center gap-1.5 transition-colors"
                >
                    <ArrowLeft size={14} /> Back to Tasks
                </button>
            </div>
        );
    }

    const isRunning = task.state === 'Running';

    const handleStop = () => {
        if (window.confirm(`Stop task "${task.name}"?`)) {
            stopMutation.mutate();
        }
    };

    return (
        <div className="flex flex-col h-full overflow-hidden">
            <div className="flex items-center gap-2 px-3 py-2.5 border-b border-zinc-800/80 bg-zinc-950 shrink-0">
                <button
                    onClick={() => void navigate({ to: '/tasks' })}
                    className="p-1.5 rounded-lg text-zinc-500 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
                >
                    <ArrowLeft size={16} />
                </button>

                <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 min-w-0">
                        <span className={cn('w-2 h-2 rounded-full shrink-0', getStatusDot(task.state), isRunning && 'animate-pulse-dot')} />
                        <h1 className="text-sm font-semibold text-zinc-100 truncate">{task.name}</h1>
                        <span className={cn('text-[10px] font-medium px-1.5 py-0.5 rounded-full ring-1 ring-inset shrink-0', getStatusBgColor(task.state))}>
                            {task.state}
                        </span>
                    </div>
                    <div className="mt-0.5 ml-4">
                        <span className="text-[11px] text-zinc-600 font-mono">{task.profile}</span>
                    </div>
                </div>

                {isRunning && (
                    <button
                        onClick={handleStop}
                        className="flex items-center gap-1 px-2.5 py-1.5 text-xs rounded-lg border border-red-900/30 bg-red-950/20 text-red-500 hover:bg-red-950/40 transition-colors shrink-0"
                    >
                        <Square size={11} />
                        Stop
                    </button>
                )}
            </div>

            <StatsPanel task={task} />
            <EventStream events={taskEvents} />

            {isRunning && (
                <InputBar taskId={task.id} onStop={handleStop} />
            )}
        </div>
    );
}
