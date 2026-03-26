import { useNavigate, useParams } from '@tanstack/react-router';
import { ArrowLeft, Square, ChevronRight } from 'lucide-react';
import { useQuery, useMutation } from '@tanstack/react-query';
import { stopTask } from '../../api/client';
import { cn, getStatusDot } from '../../lib/utils';
import { StatsPanel } from './StatsPanel';
import { EventStream } from './EventStream';
import { InputBar } from './InputBar';
import { trpc } from '../../api/trpc';
import type { OrchestratorEvent } from '../../types';

export function SessionViewer() {
    const params = useParams({ strict: false }) as { id?: string };
    const id = params.id ?? '';
    const navigate = useNavigate();

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
                <div className="w-10 h-10 rounded-xl bg-zinc-900 border border-zinc-800 flex items-center justify-center mb-1">
                    <Square size={18} className="text-zinc-700" />
                </div>
                <p className="text-sm font-medium text-zinc-500">Task not found</p>
                <button
                    onClick={() => void navigate({ to: '/tasks' })}
                    className="flex items-center gap-1.5 text-xs text-zinc-600 hover:text-zinc-300 border border-zinc-800 hover:border-zinc-700 px-3 py-1.5 rounded-lg transition-colors"
                >
                    <ArrowLeft size={12} /> Back to Tasks
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
            {/* Top bar */}
            <div className="flex items-center gap-2 px-3 py-2 border-b border-zinc-800/60 bg-zinc-950 shrink-0">
                <button
                    onClick={() => void navigate({ to: '/tasks' })}
                    className="p-1.5 rounded-md text-zinc-600 hover:text-zinc-300 hover:bg-zinc-800/60 transition-colors shrink-0"
                    aria-label="Back to tasks"
                >
                    <ArrowLeft size={15} />
                </button>

                <ChevronRight size={13} className="text-zinc-700 shrink-0" />

                {/* Task name + status */}
                <div className="flex items-center gap-2 min-w-0 flex-1">
                    <span
                        className={cn(
                            'w-1.5 h-1.5 rounded-full shrink-0',
                            getStatusDot(task.state),
                            isRunning && 'animate-pulse-dot',
                        )}
                    />
                    <h1 className="text-sm font-semibold text-zinc-100 truncate">{task.name}</h1>
                    <span className={cn(
                        'text-[10px] font-semibold px-1.5 py-px rounded-full border shrink-0',
                        isRunning
                            ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20'
                            : task.state === 'Hibernated'
                                ? 'bg-amber-500/10 text-amber-400 border-amber-500/20'
                                : 'bg-zinc-800 text-zinc-500 border-zinc-700/50',
                    )}>
                        {task.state}
                    </span>
                    <span className="text-[11px] text-zinc-600 font-mono hidden sm:block shrink-0">{task.profile}</span>
                </div>

                {isRunning && (
                    <button
                        onClick={handleStop}
                        className="flex items-center gap-1.5 px-2.5 py-1.5 text-xs rounded-lg border border-red-900/30 bg-red-950/20 text-red-500 hover:bg-red-950/40 hover:border-red-900/50 transition-colors shrink-0"
                    >
                        <Square size={11} />
                        Stop
                    </button>
                )}
            </div>

            <StatsPanel task={task} />
            <EventStream events={taskEvents} />

            {isRunning && <InputBar taskId={task.id} onStop={handleStop} />}
        </div>
    );
}
